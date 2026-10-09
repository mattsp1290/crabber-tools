//! Standard workspace tools as one Crabber extension, with explicit host policies.
#![doc = include_str!("../../../README.md")]
use async_trait::async_trait;
use crabber::{
    ExtensionError, ToolDefinition,
    core::ToolInfo,
    extension::{Extension, Registrar},
};
use crabber_tools_core::{
    CONFIG_IDENTITY_VERSION, Capacity, Limits, WorkspaceRoot, config_hash, schema_hash,
};
use crabber_tools_search::SearchPolicy;
use crabber_tools_shell::ShellPolicy;
use crabber_tools_trackerwrite::TrackerPolicy;
use crabber_tools_urlfetch::UrlFetchPolicy;
use std::sync::Arc;
mod ids;
pub mod prelude;
pub use ids::*;
/// A fully constructed tool and its deterministic metadata.
pub struct Definition {
    /// Canonical catalog id.
    pub id: ToolId,
    /// Model-facing information.
    pub info: ToolInfo,
    /// Versioned schema identity.
    pub schema_hash: String,
    /// Native executor.
    pub definition: Arc<ToolDefinition>,
}
/// Root-free, deterministic metadata in canonical id order.
pub fn metadata() -> Vec<(ToolId, ToolInfo, String)> {
    ToolId::ALL
        .into_iter()
        .map(|id| {
            let info = match id {
                ToolId::FileRead => crabber_tools_fileops::read::info(),
                ToolId::FileWrite => crabber_tools_fileops::write::info(),
                ToolId::FileEdit => crabber_tools_fileops::edit::info(),
                ToolId::FileList => crabber_tools_fileops::list::info(),
                ToolId::Search => crabber_tools_search::info(),
                ToolId::Shell => crabber_tools_shell::info(),
                ToolId::Glob => crabber_tools_glob::info(),
                ToolId::ApplyPatch => crabber_tools_applypatch::info(),
                ToolId::UrlFetch => crabber_tools_urlfetch::info(),
                ToolId::TrackerWrite => crabber_tools_trackerwrite::info(),
            };
            let hash = schema_hash(&info);
            (id, info, hash)
        })
        .collect()
}
/// Explicit policies for a single admitted workspace mount.
pub struct Options {
    /// Root shared by every executor.
    pub root: Arc<WorkspaceRoot>,
    /// Register only this subset; cannot be empty.
    pub enabled: EnabledSet,
    /// Run-wide restriction: also denies other extensions and host tools. Usually false.
    pub restrict_to_enabled: bool,
    /// Host-owned shell policy.
    pub shell: ShellPolicy,
    /// Host-owned ripgrep policy.
    pub search: SearchPolicy,
    /// Explicit URL policy; required when UrlFetch is enabled.
    pub url_fetch: Option<UrlFetchPolicy>,
    /// Explicit bn backend policy; required when TrackerWrite is enabled.
    pub tracker: Option<TrackerPolicy>,
    /// Shared finite bounds for all tools.
    pub limits: Limits,
}
/// One extension for one root. Different roots on one agent collide on tool names.
pub struct StandardTools {
    definitions: Vec<Definition>,
    hash: String,
    restrict: bool,
}
impl StandardTools {
    /// Validate all policies and build a shared capacity plus process-global root lock.
    pub fn new(options: Options) -> Result<Self, ExtensionError> {
        options.shell.validate()?;
        options.search.validate()?;
        options.limits.validate()?;
        if let Some(policy) = &options.url_fetch {
            policy.validate()?;
        }
        if let Some(policy) = &options.tracker {
            policy.validate()?;
        }
        if options.enabled.contains(ToolId::UrlFetch) && options.url_fetch.is_none() {
            return Err(ExtensionError::Plan("url_fetch policy is required".into()));
        }
        if options.enabled.contains(ToolId::TrackerWrite) && options.tracker.is_none() {
            return Err(ExtensionError::Plan(
                "tracker_write policy is required".into(),
            ));
        }
        let capacity = Capacity::new(&options.limits)?;
        let files = Arc::new(crabber_tools_fileops::Options {
            root: options.root.clone(),
            limits: options.limits.clone(),
            capacity: capacity.clone(),
            pre_write: None,
        });
        let search = Arc::new(crabber_tools_search::Options {
            root: options.root.clone(),
            limits: options.limits.clone(),
            capacity: capacity.clone(),
            policy: options.search.clone(),
        });
        let shell = Arc::new(crabber_tools_shell::Options {
            root: options.root.clone(),
            limits: options.limits.clone(),
            capacity: capacity.clone(),
            policy: options.shell.clone(),
        });
        let mut definitions = vec![];
        for (id, info, hash) in metadata() {
            if options.enabled.contains(id) {
                let definition = match id {
                    ToolId::FileRead => crabber_tools_fileops::read::definition(files.clone()),
                    ToolId::FileWrite => crabber_tools_fileops::write::definition(files.clone()),
                    ToolId::FileEdit => crabber_tools_fileops::edit::definition(files.clone()),
                    ToolId::FileList => crabber_tools_fileops::list::definition(files.clone()),
                    ToolId::Search => crabber_tools_search::definition(search.clone())?,
                    ToolId::Shell => crabber_tools_shell::definition(shell.clone())?,
                    ToolId::Glob => {
                        crabber_tools_glob::definition(Arc::new(crabber_tools_glob::Options {
                            root: options.root.clone(),
                            limits: options.limits.clone(),
                            capacity: capacity.clone(),
                        }))
                    }
                    ToolId::ApplyPatch => crabber_tools_applypatch::definition(Arc::new(
                        crabber_tools_applypatch::Options {
                            root: options.root.clone(),
                            limits: options.limits.clone(),
                            capacity: capacity.clone(),
                        },
                    )),
                    ToolId::UrlFetch => crabber_tools_urlfetch::definition(Arc::new(
                        crabber_tools_urlfetch::Options {
                            root: options.root.clone(),
                            limits: options.limits.clone(),
                            capacity: capacity.clone(),
                            policy: options.url_fetch.clone().ok_or_else(|| {
                                ExtensionError::Plan("url_fetch policy is required".into())
                            })?,
                        },
                    ))?,
                    ToolId::TrackerWrite => crabber_tools_trackerwrite::definition(Arc::new(
                        crabber_tools_trackerwrite::Options {
                            root: options.root.clone(),
                            limits: options.limits.clone(),
                            capacity: capacity.clone(),
                            policy: options.tracker.clone().ok_or_else(|| {
                                ExtensionError::Plan("tracker_write policy is required".into())
                            })?,
                        },
                    ))?,
                };
                definitions.push(Definition {
                    id,
                    info,
                    schema_hash: hash,
                    definition,
                });
            }
        }
        if definitions.is_empty() {
            return Err(ExtensionError::Plan("enabled set is empty".into()));
        }
        let identities: Vec<_> = definitions
            .iter()
            .map(|d| (d.id.id(), &d.schema_hash))
            .collect();
        let hash = config_hash(&(
            CONFIG_IDENTITY_VERSION,
            env!("CARGO_PKG_VERSION"),
            identities,
            &options.shell,
            &options.search,
            &options.url_fetch,
            &options.tracker,
            &options.limits,
            options.root.path(),
            options.restrict_to_enabled,
        ));
        Ok(Self {
            definitions,
            hash,
            restrict: options.restrict_to_enabled,
        })
    }
    /// Enabled tools in canonical registration order.
    pub fn definitions(&self) -> &[Definition] {
        &self.definitions
    }
}
#[async_trait]
impl Extension for StandardTools {
    fn id(&self) -> &str {
        "crabber-tools/standard"
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    fn config_hash(&self) -> String {
        self.hash.clone()
    }
    async fn install(&self, r: &mut Registrar) -> Result<(), ExtensionError> {
        for d in &self.definitions {
            r.tool(d.definition.clone());
        }
        if self.restrict {
            r.restrict_tools(self.definitions.iter().map(|d| d.info.name.clone()));
        }
        Ok(())
    }
}
