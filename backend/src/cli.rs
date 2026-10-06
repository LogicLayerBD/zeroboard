//! Command-line entry points. With no arguments the binary runs the server.

use std::path::PathBuf;

const USAGE: &str = "usage: zeroboard [backup --output <file> | restore --input <file>]";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Serve,
    /// Online snapshot of the database (safe while the server is running).
    Backup { output: PathBuf },
    /// Replaces the database with a verified backup. The server must be stopped.
    Restore { input: PathBuf },
}

pub fn parse(args: impl IntoIterator<Item = String>) -> anyhow::Result<Command> {
    let args: Vec<String> = args.into_iter().collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => Ok(Command::Serve),
        ["backup", "--output", path] => Ok(Command::Backup { output: PathBuf::from(path) }),
        ["restore", "--input", path] => Ok(Command::Restore { input: PathBuf::from(path) }),
        _ => anyhow::bail!("{USAGE}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> anyhow::Result<Command> {
        parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn no_arguments_runs_the_server() {
        assert_eq!(parse_strs(&[]).unwrap(), Command::Serve);
    }

    #[test]
    fn parses_backup_and_restore() {
        assert_eq!(
            parse_strs(&["backup", "--output", "b.db"]).unwrap(),
            Command::Backup { output: PathBuf::from("b.db") }
        );
        assert_eq!(
            parse_strs(&["restore", "--input", "b.db"]).unwrap(),
            Command::Restore { input: PathBuf::from("b.db") }
        );
    }

    #[test]
    fn rejects_unknown_or_incomplete_commands_with_usage() {
        for args in [
            vec!["backup"],
            vec!["backup", "b.db"],
            vec!["restore", "--output", "b.db"],
            vec!["serve"],
            vec!["backup", "--output", "b.db", "extra"],
        ] {
            let err = parse_strs(&args).unwrap_err();
            assert!(err.to_string().contains("usage:"), "{args:?}");
        }
    }
}
