//! Shared command inputs and injectable output writers.

use std::io::Write;
use std::path::PathBuf;

use tkm_config::config::{EffectiveConfig, LoadInputs};
use tkm_config::paths::Paths;
use tkm_config::repo::{IdentitySource, RepoIdentity, repo_identity};

use crate::error::CliError;

/// State resolved once for one CLI invocation.
///
/// Construct with [`Self::new`] for in-process tests, or [`Self::from_env`] at the
/// binary boundary. Construction does not create files or open the database.
pub struct Ctx<'a> {
    /// Resolved data, configuration, and socket locations.
    pub paths: Paths,
    /// Working directory of this invocation.
    pub cwd: PathBuf,
    /// Repository identity shared with the daemon and related worktrees.
    pub repo: RepoIdentity,
    /// Captured UTF-8 environment for configuration and command decisions.
    pub env: Vec<(String, String)>,
    /// Writer for command data.
    pub stdout: &'a mut dyn Write,
    /// Writer for diagnostics and progress.
    pub stderr: &'a mut dyn Write,
}

impl<'a> Ctx<'a> {
    /// Construct from explicit locations, identity, and writers without environment access.
    ///
    /// The environment starts empty; tests can populate [`Self::env`] explicitly.
    #[must_use]
    pub fn new(
        paths: Paths,
        cwd: PathBuf,
        repo: RepoIdentity,
        stdout: &'a mut dyn Write,
        stderr: &'a mut dyn Write,
    ) -> Self {
        Self {
            paths,
            cwd,
            repo,
            env: Vec::new(),
            stdout,
            stderr,
        }
    }

    /// Resolve the same platform/XDG paths and repository rules as `tkmd`.
    ///
    /// `TKM_CONFIG` overrides the global configuration path, and repository
    /// discovery respects `HOME` and `GIT_CEILING_DIRECTORIES`.
    ///
    /// # Errors
    /// Returns an error if application paths or the working directory cannot be resolved.
    pub fn from_env(
        stdout: &'a mut dyn Write,
        stderr: &'a mut dyn Write,
    ) -> Result<Self, CliError> {
        let mut paths = Paths::from_env().ok_or(CliError::PathsUnavailable)?;
        if let Some(path) = std::env::var_os("TKM_CONFIG") {
            paths.global_config = path.into();
        }
        let cwd = std::env::current_dir()?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let ceilings = std::env::var("GIT_CEILING_DIRECTORIES").unwrap_or_default();
        let repo = repo_identity(&cwd, &ceilings, home.as_deref());
        let mut ctx = Self::new(paths, cwd, repo, stdout, stderr);
        ctx.env = std::env::vars_os()
            .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        Ok(ctx)
    }

    /// Load layered configuration using this context's captured inputs.
    ///
    /// Like the daemon, exclude its transport/delegation overrides from config-key validation.
    /// The returned config retains nonfatal diagnostics in its `errors` and `ignored` fields.
    ///
    /// # Errors
    /// Returns an error when a configuration file cannot be read or parsed.
    pub fn load_config(&self) -> Result<EffectiveConfig, CliError> {
        self.load_config_for_repo(
            (self.repo.how != IdentitySource::Dir).then(|| self.repo.root.clone()),
        )
    }

    pub(crate) fn load_config_for_repo(
        &self,
        repo_root: Option<PathBuf>,
    ) -> Result<EffectiveConfig, CliError> {
        let inputs = LoadInputs {
            global_path: self.paths.global_config.clone(),
            repo_root,
            env: self
                .env
                .iter()
                .filter(|(name, _)| name != "TKM_DECIDE_ENDPOINT" && name != "TKM_DELEGATED")
                .cloned()
                .collect(),
            flags: Vec::new(),
        };
        Ok(tkm_config::config::load(&inputs)?)
    }
}
