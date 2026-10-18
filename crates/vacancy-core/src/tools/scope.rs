//! Exact tool scopes are immutable Rust values. They are not deserializable command DTOs and confer no authority
//! to bypass the filesystem Guard. No native cleanup adapter is enabled by this preparatory module.

use super::LocalEndpoint;
use super::discovery::{Availability, Docker, DockerImage, SimulatorDevice, Simulators};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScopeError {
    #[error("discovery was unavailable or incomplete")]
    Unavailable,
    #[error("the selection is empty, duplicated, or includes an undiscovered target")]
    Selection,
    #[error("the tool identity or selected target changed after approval")]
    Changed,
}

/// A content-addressed exact image selection. It cannot grow when new images appear in Docker.
#[derive(Clone, Debug)]
pub struct ImageScope {
    endpoint: LocalEndpoint,
    daemon: String,
    images: Vec<DockerImage>,
}

impl ImageScope {
    pub fn freeze(discovered: &Docker, ids: &[String]) -> Result<Self, ScopeError> {
        if !matches!(
            discovered.availability,
            Availability::Detected | Availability::Ready
        ) {
            return Err(ScopeError::Unavailable);
        }
        let endpoint = discovered
            .endpoint
            .as_deref()
            .and_then(LocalEndpoint::parse)
            .ok_or(ScopeError::Unavailable)?;
        let daemon = discovered
            .daemon_id
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or(ScopeError::Unavailable)?;
        let images = select(&discovered.unused_images, ids, |image| &image.id)?;
        if images
            .iter()
            .any(|image| !super::discovery::image_id(&image.id))
        {
            return Err(ScopeError::Selection);
        }
        Ok(Self {
            endpoint,
            daemon,
            images,
        })
    }
    pub fn images(&self) -> &[DockerImage] {
        &self.images
    }
    pub fn endpoint(&self) -> &LocalEndpoint {
        &self.endpoint
    }
    pub fn daemon_id(&self) -> &str {
        &self.daemon
    }

    /// A newly unused image is never added. A selected image now used by a container, missing, or measured
    /// differently is skipped rather than silently replacing the approved target.
    pub fn revalidate(&self, current: &Docker) -> Result<(), ScopeError> {
        if !matches!(
            current.availability,
            Availability::Detected | Availability::Ready
        ) || current.endpoint.as_deref() != Some(self.endpoint.as_str())
            || current.daemon_id.as_deref() != Some(self.daemon.as_str())
            || self
                .images
                .iter()
                .any(|image| !current.unused_images.contains(image))
        {
            Err(ScopeError::Changed)
        } else {
            Ok(())
        }
    }
}

/// Exact unavailable device ids. An installation identity must be pinned separately before this can execute;
/// this type is deliberately only a selection, never an executable simulator plan.
#[derive(Clone, Debug)]
pub struct DeviceSelection {
    devices: Vec<SimulatorDevice>,
}
impl DeviceSelection {
    pub fn freeze(discovered: &Simulators, ids: &[String]) -> Result<Self, ScopeError> {
        if discovered.availability != Availability::Ready {
            return Err(ScopeError::Unavailable);
        }
        Ok(Self {
            devices: select(&discovered.unavailable_devices, ids, |device| &device.uuid)?,
        })
    }
    pub fn devices(&self) -> &[SimulatorDevice] {
        &self.devices
    }
    pub fn revalidate(&self, current: &Simulators) -> Result<(), ScopeError> {
        if current.availability != Availability::Ready
            || self
                .devices
                .iter()
                .any(|device| !current.unavailable_devices.contains(device))
        {
            Err(ScopeError::Changed)
        } else {
            Ok(())
        }
    }
}

fn select<T: Clone>(
    candidates: &[T],
    ids: &[String],
    key: impl Fn(&T) -> &String,
) -> Result<Vec<T>, ScopeError> {
    if ids.is_empty() || ids.len() > 10_000 {
        return Err(ScopeError::Selection);
    }
    let mut seen = std::collections::HashSet::new();
    ids.iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err(ScopeError::Selection);
            }
            candidates
                .iter()
                .find(|c| key(c) == id)
                .cloned()
                .ok_or(ScopeError::Selection)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn docker() -> Docker {
        Docker {
            availability: Availability::Detected,
            daemon_id: Some("daemon-1".into()),
            context_name: Some("desktop".into()),
            endpoint: Some("npipe:////./pipe/docker_engine".into()),
            evidence: Vec::new(),
            unused_images: vec![DockerImage {
                id: format!("sha256:{}", "a".repeat(64)),
                unique_size: "1.23GB".into(),
                shared_size: "0B".into(),
            }],
        }
    }
    #[test]
    fn image_scope_never_grows_and_usage_or_daemon_changes_refuse() {
        let before = docker();
        let scope = ImageScope::freeze(&before, &[before.unused_images[0].id.clone()]).unwrap();
        let mut after = before.clone();
        after.unused_images.push(DockerImage {
            id: format!("sha256:{}", "b".repeat(64)),
            unique_size: "4GB".into(),
            shared_size: "0B".into(),
        });
        assert!(scope.revalidate(&after).is_ok());
        assert_eq!(scope.images().len(), 1);
        after.unused_images.remove(0);
        assert_eq!(scope.revalidate(&after), Err(ScopeError::Changed));
        after = before.clone();
        after.daemon_id = Some("other".into());
        assert_eq!(scope.revalidate(&after), Err(ScopeError::Changed));
        assert!(ImageScope::freeze(&before, &["--all".into()]).is_err());
        assert!(ImageScope::freeze(&before, &vec![before.unused_images[0].id.clone(); 2]).is_err());
    }
}
