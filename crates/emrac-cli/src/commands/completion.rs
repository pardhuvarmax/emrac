use std::io;

use clap::CommandFactory;
use clap_complete::{generate, Generator, Shell};

use crate::cli::{Cli, CompletionShell};

pub fn run(shell: CompletionShell) {
    let shell = match shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Zsh => Shell::Zsh,
        CompletionShell::Fish => Shell::Fish,
    };
    write_script(shell, &mut io::stdout());
}

fn write_script<G: Generator>(shell: G, out: &mut dyn io::Write) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "emrac", out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shell_script_mentions_every_subcommand() {
        let subcommands: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        assert!(subcommands.contains(&"completion".to_string()));
        assert!(subcommands.contains(&"config".to_string()));

        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let mut buf = Vec::new();
            write_script(shell, &mut buf);
            let script = String::from_utf8(buf).unwrap();
            assert!(!script.is_empty());
            for name in &subcommands {
                assert!(
                    script.contains(name.as_str()),
                    "{shell} script missing `{name}`"
                );
            }
        }
    }
}
