//! Workspace-scoped standard-tool construction through host admission.
use crate::{Policies, StandardTools, metadata};
use async_trait::async_trait;
use crabber::{
    ExtensionError,
    extension::{ComponentIdentity, Extension, ExtensionFactory, FactoryContext, FactoryScope},
};
use crabber_tools_core::{CONFIG_IDENTITY_VERSION, WorkspaceAdmission, config_hash};
use std::sync::Arc;

/// One logical registration, with independent tool resources per workspace.
pub struct StandardToolsFactory {
    policies: Policies,
    admission: Arc<dyn WorkspaceAdmission>,
    hash: String,
}
impl StandardToolsFactory {
    /// Validate root-free policies. Admission remains the host's authority.
    pub fn new(
        policies: Policies,
        admission: Arc<dyn WorkspaceAdmission>,
    ) -> Result<Self, ExtensionError> {
        policies.validate()?;
        let identities: Vec<_> = metadata()
            .into_iter()
            .filter(|(id, _, _)| policies.enabled.contains(*id))
            .map(|(id, _, hash)| (id.id(), hash))
            .collect();
        let hash = config_hash(&(
            CONFIG_IDENTITY_VERSION,
            env!("CARGO_PKG_VERSION"),
            identities,
            &policies.shell,
            &policies.search,
            &policies.url_fetch,
            &policies.tracker,
            &policies.user_interact,
            &policies.limits,
            policies.restrict_to_enabled,
        ));
        Ok(Self {
            policies,
            admission,
            hash,
        })
    }
}
#[async_trait]
impl ExtensionFactory for StandardToolsFactory {
    fn identity(&self) -> ComponentIdentity {
        ComponentIdentity {
            id: "crabber-tools/standard-factory".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn config_hash(&self) -> String {
        self.hash.clone()
    }
    fn scope(&self) -> FactoryScope {
        FactoryScope::Workspace
    }
    async fn create(&self, context: FactoryContext) -> Result<Arc<dyn Extension>, ExtensionError> {
        let admission = self.admission.clone();
        let workspace = context.workspace().clone();
        let root = tokio::task::spawn_blocking(move || admission.admit(&workspace))
            .await
            .map_err(|_| ExtensionError::Plan("workspace admission failed".into()))??;
        Ok(Arc::new(StandardTools::new(root, self.policies.clone())?))
    }
}
