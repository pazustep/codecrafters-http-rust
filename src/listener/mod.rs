mod handler;
mod request;

use crate::options::ServerOptions;
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

pub fn start<A: ToSocketAddrs + Send + 'static>(
    addr: A,
    options: ServerOptions,
) -> JoinHandle<Result<()>> {
    tokio::spawn(async move {
        let listener = TcpListener::bind(addr).await?;
        let mut tasks = JoinSet::new();

        while let Ok((stream, _)) = listener.accept().await {
            let options = options.clone();
            tasks.spawn(async move { handle_connection(options, stream).await });
        }

        while tasks.join_next().await.is_some() {}
        Ok(())
    })
}

async fn handle_connection(options: ServerOptions, stream: TcpStream) -> Result<()> {
    let (read_half, write_half) = stream.into_split();
    let (writer_handle, responses_tx) = start_writer(write_half);
    let reader_handle = start_reader(options, read_half, responses_tx);

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
    options: ServerOptions,
    read_half: OwnedReadHalf,
    tx: mpsc::Sender<HttpResponse>,
) -> JoinHandle<Result<()>> {
    tokio::spawn(async move { read_loop(options, read_half, tx).await })
}

async fn read_loop(
    options: ServerOptions,
    read_half: OwnedReadHalf,
    tx: mpsc::Sender<HttpResponse>,
) -> Result<()> {
    let mut reader = BufReader::new(read_half);
    let mut tasks = JoinSet::new();

    loop {
        match request::read(&mut reader).await {
            Ok(Some(request)) => {
                let tx = tx.clone();
                let options = options.clone();
                tasks.spawn(async move {
                    let response = handler::handle(options, request).await;
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
