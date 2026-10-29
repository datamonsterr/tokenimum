use crate::{AdapterError, InstallContext, InstallReport, shim};
use std::path::PathBuf;
use tkm_proto::Host;

fn paths(context: &InstallContext) -> (PathBuf, PathBuf) {
    if let Some(repo) = &context.repo_root {
        (repo.clone(), repo.join(".opencode/plugins/tokenimum.ts"))
    } else {
        (
            context.agent_home.clone(),
            context.agent_home.join("plugins/tokenimum.ts"),
        )
    }
}

pub(super) fn install(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    Ok(InstallReport {
        changed_files: shim::install(Host::Opencode, &root, &path, context.dry_run)?,
        instructions: Vec::new(),
        dry_run: context.dry_run,
    })
}

pub(super) fn uninstall(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    Ok(InstallReport {
        changed_files: shim::uninstall(Host::Opencode, &root, &path, context.dry_run)?,
        instructions: Vec::new(),
        dry_run: context.dry_run,
    })
}
