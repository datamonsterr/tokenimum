use crate::{AdapterError, InstallContext, InstallReport, shim};
use std::path::PathBuf;
use tkm_proto::Host;

const TRUST_STEP: &str = "Run /reload in Pi; project scope needs project trust.";

fn paths(context: &InstallContext) -> (PathBuf, PathBuf) {
    if let Some(repo) = &context.repo_root {
        (repo.clone(), repo.join(".pi/extensions/tokenimum.ts"))
    } else {
        (
            context.agent_home.clone(),
            context.agent_home.join("extensions/tokenimum.ts"),
        )
    }
}

pub(super) fn install(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    Ok(InstallReport {
        changed_files: shim::install(Host::Pi, &root, &path, context.dry_run)?,
        instructions: vec![TRUST_STEP.to_owned()],
        dry_run: context.dry_run,
    })
}

pub(super) fn uninstall(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    Ok(InstallReport {
        changed_files: shim::uninstall(Host::Pi, &root, &path, context.dry_run)?,
        instructions: Vec::new(),
        dry_run: context.dry_run,
    })
}
