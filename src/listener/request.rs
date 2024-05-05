use std::{collections::HashMap, fmt, io, num::ParseIntError, str::FromStr, vec};
use thiserror::Error;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

#[derive(Debug)]
pub struct HttpRequest {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: HashMap<String, Vec<String>>,
    pub body: Vec<u8>,
}

#[derive(Error, Debug)]
pub struct HttpRequestError(Option<String>);

impl From<io::Error> for HttpRequestError {
    fn from(value: io::Error) -> Self {
        println!("I/O error while parsing request: {}", value);
        Self(None)
    }
}

impl From<ParseIntError> for HttpRequestError {
    fn from(value: ParseIntError) -> Self {
        println!("error parsing value as integer: {}", value);
        Self(None)
    }
}

pub async fn read<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<HttpRequest>, HttpRequestError> {
    let mut buffer = String::new();
    let bytes_read = reader.read_line(&mut buffer).await?;

    if bytes_read == 0 {
        return Ok(None);
    }

    let request_line = buffer.trim_end().parse::<RequestLine>()?;
    let mut headers = HashMap::<String, Vec<String>>::new();

    loop {
        buffer.clear();
        reader.read_line(&mut buffer).await?;

        if buffer.trim().is_empty() {
            break;
        }

        let header = buffer.parse::<HttpHeader>()?;
        let values = headers.entry(header.key).or_default();
        values.push(header.value);
    }

    let body: Vec<u8> = if request_line.method != "GET" {
        match headers.get("content-length") {
            Some(values) => {
                let length = values.last().unwrap().parse::<usize>()?;
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body).await?;
                body
            }
            None => {
                let mut body = Vec::new();
                reader.read_to_end(&mut body).await?;
                body
            }
        }
    } else {
        vec![]
    };

    Ok(Some(HttpRequest {
        method: request_line.method,
        target: request_line.target,
        version: request_line.version,
        headers,
        body,
    }))
}

#[derive(Debug)]
struct RequestLine {
    method: String,
    target: String,
    version: String,
}

impl FromStr for RequestLine {
    type Err = HttpRequestError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(' ') {
            Some((method, rest)) => match rest.split_once(' ') {
                Some((target, version)) => Ok(Self {
                    method: method.to_string(),
                    target: target.to_string(),
                    version: version.to_string(),
                }),
                None => {
                    println!("second SP not found in request line: {}", s);
                    Err(HttpRequestError(None))
                }
            },
            None => {
                println!("first SP not found in request line: {}", s);
                Err(HttpRequestError(None))
            }
        }
    }
}

impl fmt::Display for HttpRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(message) => write!(f, "HTTP 400 {}", message),
            None => write!(f, "HTTP 400 Bad Request"),
        }
    }
}

struct HttpHeader {
    key: String,
    value: String,
}

impl FromStr for HttpHeader {
    type Err = HttpRequestError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(':') {
            Some((key, value)) => Ok(Self {
                key: key.to_lowercase(),
                value: value.trim().to_string(),
            }),
            _ => {
                println!(": not found in HTTP header line");
                Err(HttpRequestError(None))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::BufReader;

    use super::*;

    #[test]
    fn parse_request_line_ok() {
        let line = "GET / HTTP/1.1".parse::<RequestLine>().unwrap();
        assert_eq!(line.method, "GET");
        assert_eq!(line.target, "/");
        assert_eq!(line.version, "HTTP/1.1");
    }

    #[test]
    fn parse_request_line_no_sp() {
        match "GET".parse::<RequestLine>() {
            Err(_) => {}
            other => panic!("expected parse error, got {:?}", other),
        }
    }

    #[test]
    fn parse_header_ok() {
        let HttpHeader { key, value } = "Host: localhost:4221".parse().unwrap();
        assert_eq!(key, "host");
        assert_eq!(value, "localhost:4221");
    }

    #[tokio::test]
    async fn parse_full_request() {
        let request_str = concat!(
            "GET /index.html HTTP/1.1\r\n",
            "Host: localhost:4221\r\n",
            "User-Agent: curl/7.64.1\r\n",
            "\r\n"
        );

        let mut reader = BufReader::new(request_str.as_bytes());
        let request = super::read(&mut reader).await.unwrap().unwrap();

        assert_eq!(request.method, "GET");
        assert_eq!(request.target, "/index.html");
        assert_eq!(request.version, "HTTP/1.1");

        let headers = request.headers;
        assert_eq!(headers.len(), 2);
        assert_eq!(
            headers.get("host"),
            Some(&vec!["localhost:4221".to_string()])
        );
        assert_eq!(
            headers.get("user-agent"),
            Some(&vec!["curl/7.64.1".to_string()])
        );

        let body = request.body;
        assert_eq!(body.len(), 0);
    }

    #[tokio::test]
    async fn parse_empty_request() {
        let source: Vec<u8> = vec![];
        let mut reader = BufReader::new(source.as_slice());
        let request = super::read(&mut reader).await.unwrap();
        assert!(request.is_none());
    }
}
