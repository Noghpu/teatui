use std::path::PathBuf;

use crate::command::ExternalCommand;
use crate::config::Config;

#[derive(Debug, Clone)]
pub struct TeaClient {
    program: String,
}

impl TeaClient {
    pub fn new(config: &Config) -> Self {
        Self {
            program: config.commands.tea.clone(),
        }
    }

    pub fn version_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(self.program.clone(), ["--version"], cwd)
    }

    pub fn login_list_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(self.program.clone(), ["login", "list"], cwd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_version_command_argv() {
        let config = Config::default();
        let client = TeaClient::new(&config);
        let command = client.version_command("C:/repo");

        assert_eq!(command.program, "tea");
        assert_eq!(command.args, vec!["--version"]);
        assert_eq!(command.cwd, PathBuf::from("C:/repo"));
    }

    #[test]
    fn builds_login_list_command_argv() {
        let config = Config::default();
        let client = TeaClient::new(&config);
        let command = client.login_list_command("C:/repo");

        assert_eq!(command.program, "tea");
        assert_eq!(command.args, vec!["login", "list"]);
        assert_eq!(command.cwd, PathBuf::from("C:/repo"));
    }
}
