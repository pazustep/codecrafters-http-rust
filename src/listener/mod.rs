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
            let line = format!(
                "HTTP/1.1 {} {}\r\n",
                response.status_code, response.status_line
            );

            writer.write_all(line.as_bytes()).await?;

            writer
                .write_all("content-length: 0\r\n\r\n".as_bytes())
                .await?;

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
                let response = HttpResponse {
                    status_code: 400,
                    status_line: format!("{}", error),
                };

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
}

async fn process_request(request: HttpRequest) -> HttpResponse {
    match request.target.as_str() {
        "/" => HttpResponse {
            status_code: 200,
            status_line: "OK".to_string(),
        },
        _ => HttpResponse {
            status_code: 404,
            status_line: "Not Found".to_string(),
        },
    }
}
