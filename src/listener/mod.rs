mod request;

use self::request::HttpRequest;
use std::io::Result;
use tokio::{
    io::{AsyncWriteExt, BufReader, BufWriter},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream, ToSocketAddrs,
    },
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

pub fn start<A: ToSocketAddrs + Send + 'static>(addr: A) -> JoinHandle<Result<()>> {
    tokio::spawn(async move {
        let listener = TcpListener::bind(addr).await?;
        let mut tasks = JoinSet::new();

        while let Ok((stream, _)) = listener.accept().await {
            tasks.spawn(async move { handle_connection(stream).await });
        }

        while tasks.join_next().await.is_some() {}
        Ok(())
    })
}

async fn handle_connection(stream: TcpStream) -> Result<()> {
    let (read_half, write_half) = stream.into_split();
    let (writer_handle, responses_tx) = start_writer(write_half);
    let reader_handle = start_reader(read_half, responses_tx);

    let _ = tokio::join!(writer_handle, reader_handle);

    Ok(())
}

fn start_writer(
    write_half: OwnedWriteHalf,
) -> (JoinHandle<Result<()>>, mpsc::Sender<HttpResponse>) {
    let (tx, mut rx) = mpsc::channel::<HttpResponse>(5);

    let handle = tokio::spawn(async move {
        let mut writer = BufWriter::new(write_half);

        while let Some(response) = rx.recv().await {
            let status = format!(
                "HTTP/1.1 {} {}\r\n",
                response.status_code, response.status_line
            );

            writer.write_all(status.as_bytes()).await?;

            if !response.has_content_length() {
                let header = format!("content-length: {}\r\n", response.content.len());
                writer.write_all(header.as_bytes()).await?;
            }

            for (key, value) in response.headers {
                let header = format!("{}: {}\r\n", key, value);
                writer.write_all(header.as_bytes()).await?;
            }

            writer.write_all("\r\n".as_bytes()).await?;
            writer.write_all(response.content.as_slice()).await?;
            writer.flush().await?;
        }

        Ok(())
    });

    (handle, tx)
}

fn start_reader(
    read_half: OwnedReadHalf,
    tx: mpsc::Sender<HttpResponse>,
) -> JoinHandle<Result<()>> {
    tokio::spawn(async move { read_loop(read_half, tx).await })
}

async fn read_loop(read_half: OwnedReadHalf, tx: mpsc::Sender<HttpResponse>) -> Result<()> {
    let mut reader = BufReader::new(read_half);
    let mut tasks = JoinSet::new();

    loop {
        match request::read(&mut reader).await {
            Ok(Some(request)) => {
                let tx = tx.clone();
                tasks.spawn(async move {
                    let response = process_request(request).await;
                    let _ = tx.send(response).await;
                });
            }
            Ok(None) => {
                break;
            }
            Err(error) => {
                let response = HttpResponse::status(400, error.to_string());
                let _ = tx.send(response).await;
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
struct HttpResponse {
    status_code: u16,
    status_line: String,
    headers: Vec<(String, String)>,
    content: Vec<u8>,
}

impl HttpResponse {
    fn status<T: Into<String>>(status_code: u16, status_line: T) -> Self {
        Self {
            status_code,
            status_line: status_line.into(),
            headers: vec![],
            content: vec![],
        }
    }

    fn ok<T: Into<String>>(content_type: T, content: Vec<u8>) -> Self {
        Self {
            status_code: 200,
            status_line: "OK".to_string(),
            headers: vec![("content-type".to_string(), content_type.into())],
            content,
        }
    }

    fn has_content_length(&self) -> bool {
        self.headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("content-length"))
    }
}

async fn process_request(request: HttpRequest) -> HttpResponse {
    match request.target.as_str() {
        "/" => HttpResponse::status(200, "OK"),
        "/user-agent" => match request.headers.get("user-agent").map(|v| v.as_slice()) {
            Some([user_agent, ..]) => {
                HttpResponse::ok("text/plain", user_agent.as_bytes().to_vec())
            }
            _ => HttpResponse::status(400, "Bad Request"),
        },
        path if path.starts_with("/echo/") => {
            let message = &path[6..];
            let content = message.as_bytes().to_vec();
            HttpResponse::ok("text/plain", content)
        }
        _ => HttpResponse::status(404, "Not Found"),
    }
}
