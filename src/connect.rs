use async_std::io as aio;
use std::error::Error;

pub struct Connect {
	pub send: async_process::ChildStdin,
	pub recv: async_std::io::BufReader<async_process::ChildStdout>,
}

pub async fn connect(dir: &str) -> Result<Connect, Box<dyn Error>> {
	let mut child: async_process::Child;
	if let Some(colon_pos) =
		if &dir[..1] == "/" || &dir[..1] == "." || &dir[..1] == "~" { None } else { dir.find(':') }
	{
		let host = &dir[..colon_pos];
		let path = &dir[colon_pos + 1..];
		println!("Connecting {} : {}", &host, &path);
		child = async_process::Command::new("ssh")
			.arg(host)
			.arg("syncr")
			.arg("serve")
			.arg(path)
			.stdin(async_process::Stdio::piped())
			.stdout(async_process::Stdio::piped())
			.spawn()
			.map_err(|e| format!("Failed to spawn SSH subprocess for {}:{}: {}", host, path, e))?;
	} else {
		child = async_process::Command::new("syncr")
			.arg("serve")
			.arg(dir)
			.stdin(async_process::Stdio::piped())
			.stdout(async_process::Stdio::piped())
			.spawn()
			.map_err(|e| format!("Failed to spawn local subprocess for {}: {}", dir, e))?;
	}
	let send = child.stdin.take().ok_or("Failed to acquire stdin from subprocess")?;
	let recv =
		aio::BufReader::new(child.stdout.take().ok_or("Failed to acquire stdout from subprocess")?);
	Ok(Connect { send, recv })
}
