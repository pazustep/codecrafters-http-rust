use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct ServerOptions {
    pub root: Option<PathBuf>,
}

impl ServerOptions {
    pub fn new() -> Self {
        let args = env::args().collect::<Vec<_>>();
        let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        match args[1..] {
            ["--directory", directory] => {
                let path: &Path = directory.as_ref();
                match path.canonicalize() {
                    Ok(path) if path.is_dir() => {
                        println!("serving files from directory {:?}", path);
                        Self { root: Some(path) }
                    }
                    Ok(path) => {
                        println!(
                            "{path:?} does not exist or is not a directory; file serving disabled"
                        );
                        Self { root: None }
                    }
                    Err(err) => {
                        println!(
                            "failed to canonicalize directory, file serving disabled: {}",
                            err
                        );
                        Self { root: None }
                    }
                }
            }
            _ => {
                println!("--directory not provided, file serving disabled");
                Self { root: None }
            }
        }
    }
}
