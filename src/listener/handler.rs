use crate::{
    listener::{request::HttpRequest, HttpResponse},
    options::ServerOptions,
};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
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
            HttpResponse::ok("text/plain", content)
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
                .write(true)
                .mode(0o600)
                .open(&path)
                .await;

            match open_result {
                Ok(mut file) => match file.write_all(request.body.as_slice()).await {
                    Ok(_) => HttpResponse::status(201, "Accepted"),
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
