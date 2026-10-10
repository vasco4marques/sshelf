use std::os::unix::process::CommandExt;
use std::process::{Command, exit};
use std::{env, fs, io};

use inquire::{InquireError, Select};

/// Collects connectable aliases from `Host` lines, skipping wildcard patterns.
fn parse_hosts(config: &str) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for line in config.lines() {
        let mut words = line.split_whitespace();
        let Some(keyword) = words.next() else { continue };
        if !keyword.eq_ignore_ascii_case("host") {
            continue;
        }
        for alias in words {
            let is_pattern = alias.contains(['*', '?']) || alias.starts_with('!');
            if !is_pattern && !hosts.iter().any(|h| h == alias) {
                hosts.push(alias.to_string());
            }
        }
    }
    hosts
}

fn main() {
    let home = env::var("HOME").unwrap_or_else(|_| {
        eprintln!("sshelf: HOME is not set");
        exit(1);
    });
    let path = format!("{home}/.ssh/config");

    let config = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            eprintln!("sshelf: cannot read {path}: {e}");
            exit(1);
        }
    };

    let hosts = parse_hosts(&config);
    if hosts.is_empty() {
        eprintln!(
            "No ssh configurations found in ~/.ssh/config, use sshelf --configurations to add some"
        );
        exit(1);
    }

    let alias = match Select::new("Connect to:", hosts).prompt() {
        Ok(alias) => alias,
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => exit(0),
        Err(e) => {
            eprintln!("sshelf: {e}");
            exit(1);
        }
    };

    // exec only returns if ssh could not be started.
    let err = Command::new("ssh").arg(&alias).exec();
    eprintln!("sshelf: failed to run ssh: {err}");
    exit(1);
}

#[cfg(test)]
mod tests {
    use super::parse_hosts;

    #[test]
    fn keeps_concrete_aliases_only() {
        let config = "\
Host web1 web2
    HostName 10.0.0.1
host db1 *.corp
  HOST indented
Host * !bastion jump?
Hostname notahost
Host web1
";
        assert_eq!(parse_hosts(config), ["web1", "web2", "db1", "indented"]);
    }
}
