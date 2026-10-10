mod configuration;

use std::os::unix::process::CommandExt;
use std::process::{Command, exit};
use std::{env, fs, io};

use inquire::{InquireError, Select};

const USAGE: &str = "usage: sshelf [--configurations]";

/// Returns the first word of a line if it is a block keyword (`Host`/`Match`), lowercased.
fn block_keyword(line: &str) -> Option<String> {
    let word = line.split_whitespace().next()?.to_ascii_lowercase();
    (word == "host" || word == "match").then_some(word)
}

fn is_pattern(alias: &str) -> bool {
    alias.contains(['*', '?']) || alias.starts_with('!')
}

/// Collects connectable aliases from `Host` lines, skipping wildcard patterns.
fn parse_hosts(config: &str) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for line in config.lines() {
        if block_keyword(line).as_deref() != Some("host") {
            continue;
        }
        for alias in line.split_whitespace().skip(1) {
            if !is_pattern(alias) && !hosts.iter().any(|h| h == alias) {
                hosts.push(alias.to_string());
            }
        }
    }
    hosts
}

/// Exits quietly on Esc/Ctrl+C, with an error otherwise.
fn prompt_failed(e: InquireError) -> ! {
    match e {
        InquireError::OperationCanceled | InquireError::OperationInterrupted => exit(0),
        e => {
            eprintln!("sshelf: {e}");
            exit(1);
        }
    }
}

fn pick_host(config: &str, prompt: &str) -> String {
    let hosts = parse_hosts(config);
    if hosts.is_empty() {
        eprintln!(
            "No ssh configurations found in ~/.ssh/config, use sshelf --configurations to add some"
        );
        exit(1);
    }
    Select::new(prompt, hosts).prompt().unwrap_or_else(|e| prompt_failed(e))
}

fn connect(config: &str) -> ! {
    let alias = pick_host(config, "Connect to:");
    // exec only returns if ssh could not be started.
    let err = Command::new("ssh").arg(&alias).exec();
    eprintln!("sshelf: failed to run ssh: {err}");
    exit(1);
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

    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [] => connect(&config),
        [flag] if flag == "--configurations" => configuration::run(&path, &config),
        _ => {
            eprintln!("{USAGE}");
            exit(1);
        }
    }
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
