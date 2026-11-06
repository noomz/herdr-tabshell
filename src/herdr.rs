use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

pub fn call(method: &str, params: Value) -> io::Result<Value> {
    let path = std::env::var("HERDR_SOCKET_PATH")
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "HERDR_SOCKET_PATH not set"))?;
    let mut stream = UnixStream::connect(path)?;
    let request = json!({"id": "tabshell", "method": method, "params": params});
    stream.write_all(format!("{request}\n").as_bytes())?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let mut response: Value = serde_json::from_str(&line)?;
    if let Some(error) = response.get("error") {
        let code = error["code"].as_str().unwrap_or("error");
        let message = error["message"].as_str().unwrap_or("");
        return Err(io::Error::other(format!("{method}: {code}: {message}")));
    }
    Ok(response["result"].take())
}
