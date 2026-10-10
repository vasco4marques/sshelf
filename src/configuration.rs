use std::fs;
use std::process::{Command, exit};

use inquire::{Confirm, Select};

use crate::{block_keyword, pick_host, prompt_failed};

/// `sshelf --configurations`: lets the user pick an action on ~/.ssh/config.
pub fn run(path: &str, config: &str) {
    let action = Select::new("Configuration:", vec!["Delete"])
        .prompt()
        .unwrap_or_else(|e| prompt_failed(e));
    match action {
        "Delete" => delete(path, config),
        _ => unreachable!(),
    }
}

fn delete(path: &str, config: &str) {
    let alias = pick_host(config, "Delete:");
    let confirmed = Confirm::new(&format!("Delete {alias}?"))
        .with_default(false)
        .prompt()
        .unwrap_or_else(|e| prompt_failed(e));
    if !confirmed {
        return;
    }
    let Some(updated) = delete_alias(config, &alias) else {
        eprintln!("sshelf: {alias} not found in {path}");
        exit(1);
    };
    match safe_write(path, &updated, &alias) {
        Ok(()) => println!("Deleted {alias}"),
        Err(e) => {
            eprintln!("sshelf: {e}");
            exit(1);
        }
    }
}

/// Line range `start..end` of the first `Host` block naming `alias`.
/// The block runs until the next `Host`/`Match` line or the end of the file.
fn find_block(lines: &[&str], alias: &str) -> Option<(usize, usize)> {
    let start = lines.iter().position(|line| {
        block_keyword(line).as_deref() == Some("host")
            && line.split_whitespace().skip(1).any(|a| a == alias)
    })?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| block_keyword(line).is_some())
        .map_or(lines.len(), |i| start + 1 + i);
    Some((start, end))
}

/// Removes `alias` from its Host line, or the whole block if it is the only alias.
fn delete_alias(config: &str, alias: &str) -> Option<String> {
    let mut lines: Vec<&str> = config.lines().collect();
    let (start, end) = find_block(&lines, alias)?;

    let host_line = lines[start];
    let mut words = host_line.split_whitespace();
    let keyword = words.next()?;
    let others: Vec<&str> = words.filter(|a| *a != alias).collect();

    let rebuilt;
    if others.is_empty() {
        lines.drain(start..end);
    } else {
        let indent = &host_line[..host_line.len() - host_line.trim_start().len()];
        rebuilt = format!("{indent}{keyword} {}", others.join(" "));
        lines[start] = &rebuilt;
    }

    let mut out = lines.join("\n");
    if config.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    Some(out)
}

/// Writes `content` to a temp file next to `path`, checks it with `ssh -G`,
/// then renames it over `path`. On any failure `path` is left untouched.
fn safe_write(path: &str, content: &str, check_alias: &str) -> Result<(), String> {
    let tmp = format!("{path}.sshelf-tmp");
    let result = (|| {
        fs::write(&tmp, content).map_err(|e| format!("cannot write {tmp}: {e}"))?;
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())
                .map_err(|e| format!("cannot set permissions on {tmp}: {e}"))?;
        }
        let check = Command::new("ssh")
            .args(["-G", "-F", &tmp, check_alias])
            .output()
            .map_err(|e| format!("cannot run ssh: {e}"))?;
        if !check.status.success() {
            return Err(format!(
                "ssh rejected the new config, nothing was changed:\n{}",
                String::from_utf8_lossy(&check.stderr).trim_end()
            ));
        }
        fs::rename(&tmp, path).map_err(|e| format!("cannot replace {path}: {e}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::delete_alias;

    const CONFIG: &str = "\
User vini

# web servers
Host web1   web2 web3
    HostName 10.0.0.1

Host db
    HostName 10.0.0.2
    Port 2222

Match user root
    IdentityFile ~/.ssh/root
";

    #[test]
    fn delete_one_of_several_aliases_keeps_block() {
        let expected = CONFIG.replace("Host web1   web2 web3", "Host web1 web3");
        assert_eq!(delete_alias(CONFIG, "web2").unwrap(), expected);
    }

    #[test]
    fn delete_only_alias_removes_block_up_to_next_keyword() {
        let expected = CONFIG.replace("Host db\n    HostName 10.0.0.2\n    Port 2222\n\n", "");
        assert_eq!(delete_alias(CONFIG, "db").unwrap(), expected);
    }
}
