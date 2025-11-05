use std::error::Error;
use std::process::Stdio;
use tokio::io::BufReader;

pub struct Connect {
	pub send: tokio::process::ChildStdin,
	pub recv: BufReader<tokio::process::ChildStdout>,
}

pub async fn connect(dir: &str) -> Result<Connect, Box<dyn Error>> {
	let mut child: tokio::process::Child;
	if let Some(colon_pos) =
		if &dir[..1] == "/" || &dir[..1] == "." || &dir[..1] == "~" { None } else { dir.find(':') }
	{
		let host = &dir[..colon_pos];
		let path = &dir[colon_pos + 1..];
		println!("Connecting {} : {}", &host, &path);
		child = tokio::process::Command::new("ssh")
			.arg(host)
			.arg("syncr")
			.arg("serve")
			.arg(path)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.spawn()
			.map_err(|e| format!("Failed to spawn SSH subprocess for {}:{}: {}", host, path, e))?;
	} else {
		child = tokio::process::Command::new("syncr")
			.arg("serve")
			.arg(dir)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.spawn()
			.map_err(|e| format!("Failed to spawn local subprocess for {}: {}", dir, e))?;
	}
	let send = child.stdin.take().ok_or("Failed to acquire stdin from subprocess")?;
	let recv =
		BufReader::new(child.stdout.take().ok_or("Failed to acquire stdout from subprocess")?);
	Ok(Connect { send, recv })
}
