use std::process::Stdio;

use crate::{
    listener::{request::HttpRequest, HttpResponse},
    options::ServerOptions,
};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
    process::Command,
};

pub async fn handle(options: ServerOptions, request: HttpRequest) -> HttpResponse {
    match (
        request.method.as_str(),
        request.target.as_str(),
        options.root,
    ) {
        ("GET", "/", _) => HttpResponse::status(200, "OK"),
        ("GET", "/user-agent", _) => {
            match request.headers.get("user-agent").map(|v| v.as_slice()) {
                Some([user_agent, ..]) => {
                    HttpResponse::ok("text/plain", user_agent.as_bytes().to_vec())
                }
                _ => HttpResponse::status(400, "Bad Request"),
            }
        }
        ("GET", path, _) if path.starts_with("/echo/") => {
            let message = &path[6..];
            let content = message.as_bytes().to_vec();
            let is_gzip = request
                .headers
                .get("accept-encoding")
                .iter()
                .flat_map(|v| v.iter())
                .flat_map(|v| v.split(','))
                .any(|v| v.trim() == "gzip");

            if is_gzip {
                match compress(&content).await {
                    Ok(compressed) => HttpResponse::ok_with_headers(
                        compressed,
                        vec![
                            ("content-type".to_string(), "text/plain".to_string()),
                            ("content-encoding".to_string(), "gzip".to_string()),
                        ],
                    ),
                    Err(err) => {
                        eprintln!("Error compressing message {:?}: {}", message, err);
                        HttpResponse::status(500, "Internal Server Error")
                    }
                }
            } else {
                HttpResponse::ok("text/plain", content)
            }
        }
        ("GET", file, Some(root)) if file.starts_with("/files/") => {
            let path = root.join(&file[7..]);

            if path.exists() {
                match fs::read(&path).await {
                    Ok(content) => HttpResponse::ok("application/octet-stream", content),
                    Err(err) => {
                        println!("Error reading contents of file {:?}: {}", path, err);
                        HttpResponse::status(500, "Internal Server Error")
                    }
                }
            } else {
                HttpResponse::status(404, "Not Found")
            }
        }
        ("POST", file, Some(root)) if file.starts_with("/files/") => {
            let path = root.join(&file[7..]);
            let open_result = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(&path)
                .await;

            match open_result {
                Ok(mut file) => match file.write_all(request.body.as_slice()).await {
                    Ok(_) => HttpResponse::status(201, "Created"),
                    Err(err) => {
                        println!("Error writing request body to file {:?}: {}", path, err);
                        HttpResponse::status(500, "Internal Server Error")
                    }
                },
                Err(err) => {
                    println!("Error opening file {:?} for writing: {}", path, err);
                    HttpResponse::status(500, "Internal Server Error")
                }
            }
        }
        _ => HttpResponse::status(404, "Not Found"),
    }
}

async fn compress(content: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut child = Command::new("gzip")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let child_stdin = child.stdin.as_mut().unwrap();
    child_stdin.write_all(content).await?;

    let output = child.wait_with_output().await?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
}
