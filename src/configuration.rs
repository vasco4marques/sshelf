use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, exit};

use inquire::validator::Validation;
use inquire::{Confirm, Select, Text, required};

use crate::{block_keyword, is_pattern, parse_hosts, pick_host, prompt_failed};

/// `sshelf --configurations`: lets the user pick an action on ~/.ssh/config.
pub fn run(path: &str, config: &str) {
    let action = Select::new("Configuration:", vec!["Add", "Delete"])
        .prompt()
        .unwrap_or_else(|e| prompt_failed(e));
    match action {
        "Add" => add(path, config),
        "Delete" => delete(path, config),
        _ => unreachable!(),
    }
}

fn add(path: &str, config: &str) {
    let existing = parse_hosts(config);
    let alias = Text::new("Host (alias):")
        .with_validator(move |input: &str| {
            let alias = input.trim();
            Ok(if alias.is_empty() || alias.contains(char::is_whitespace) || is_pattern(alias) {
                Validation::Invalid("Alias must be one word without *, ? or a leading !".into())
            } else if existing.iter().any(|h| h == alias) {
                Validation::Invalid("Duplicated host name".into())
            } else {
                Validation::Valid
            })
        })
        .prompt()
        .unwrap_or_else(|e| prompt_failed(e));
    let alias = alias.trim();

    let mut options: Vec<(String, String)> = Vec::new();
    for key in ["HostName", "User", "Port"] {
        let value = Text::new(&format!("{key} (empty to skip):"))
            .prompt()
            .unwrap_or_else(|e| prompt_failed(e));
        if !value.trim().is_empty() {
            options.push((key.to_string(), value.trim().to_string()));
        }
    }

    let ssh_dir = Path::new(path).parent().expect("config path has a parent");
    let keys = list_keys(ssh_dir);
    if !keys.is_empty() {
        let choices: Vec<String> = ["None".to_string()].into_iter().chain(keys).collect();
        let choice = Select::new("IdentityFile:", choices)
            .raw_prompt()
            .unwrap_or_else(|e| prompt_failed(e));
        if choice.index > 0 {
            options.push(("IdentityFile".to_string(), format!("~/.ssh/{}", choice.value)));
        }
    }

    loop {
        let key = Text::new("Other option name (empty to finish):")
            .prompt()
            .unwrap_or_else(|e| prompt_failed(e));
        let key = key.trim();
        if key.is_empty() {
            break;
        }
        let value = Text::new(&format!("{key} value:"))
            .with_validator(required!())
            .prompt()
            .unwrap_or_else(|e| prompt_failed(e));
        options.push((key.to_string(), value.trim().to_string()));
    }

    let mut block = format!("Host {alias}\n");
    for (key, value) in &options {
        block.push_str(&format!("    {key} {value}\n"));
    }
    match safe_write(path, &insert_block(config, &block), alias) {
        Ok(()) => println!("Added {alias}"),
        Err(e) => {
            eprintln!("sshelf: {e}");
            exit(1);
        }
    }
}

/// Private keys in `ssh_dir`: files that have a matching `<name>.pub` next to them.
fn list_keys(ssh_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(ssh_dir) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| {
            ssh_dir.join(name).is_file() && ssh_dir.join(format!("{name}.pub")).is_file()
        })
        .collect();
    keys.sort();
    keys
}

/// Puts `block` right before the first `Host`/`Match` line, so global options at the
/// top stay global and the new block wins over later ones like `Host *`.
/// With no blocks in the file, appends it at the end.
fn insert_block(config: &str, block: &str) -> String {
    let mut offset = 0;
    for line in config.split_inclusive('\n') {
        if block_keyword(line).is_some() {
            return format!("{}{block}\n{}", &config[..offset], &config[offset..]);
        }
        offset += line.len();
    }
    let separator = match config {
        "" => "",
        c if c.ends_with("\n\n") => "",
        c if c.ends_with('\n') => "\n",
        _ => "\n\n",
    };
    format!("{config}{separator}{block}")
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
        let permissions = match fs::metadata(path) {
            Ok(meta) => meta.permissions(),
            Err(_) => fs::Permissions::from_mode(0o600),
        };
        fs::set_permissions(&tmp, permissions)
            .map_err(|e| format!("cannot set permissions on {tmp}: {e}"))?;
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
    use super::{delete_alias, insert_block};

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

    const BLOCK: &str = "Host new\n    HostName 10.0.0.9\n";

    #[test]
    fn insert_goes_after_globals_and_before_first_block() {
        let expected = CONFIG.replace("# web servers\n", &format!("# web servers\n{BLOCK}\n"));
        assert_eq!(insert_block(CONFIG, BLOCK), expected);
    }

    #[test]
    fn insert_appends_when_file_has_no_blocks() {
        assert_eq!(insert_block("User vini\n", BLOCK), format!("User vini\n\n{BLOCK}"));
        assert_eq!(insert_block("", BLOCK), BLOCK);
    }
}
