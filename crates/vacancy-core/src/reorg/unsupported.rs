use super::{Draft, Error, InventoryEntry, ReviewedMove, Status};
use crate::{CancelToken, Env};
use std::path::Path;

#[derive(Debug)]
pub struct Inventory {
    _private: (),
}
#[derive(Debug)]
pub struct ValidatedPlan {
    _private: (),
}
#[derive(Debug)]
pub struct Engine {
    _private: (),
}
impl Inventory {
    pub fn capture(_: &Env, _: &CancelToken) -> Result<Self, Error> {
        Err(Error::Unsupported)
    }
    pub fn entries(&self) -> &[InventoryEntry] {
        &[]
    }
    pub fn truncated(&self) -> bool {
        false
    }
    pub fn validate(&self, _: &[Draft], _: &CancelToken) -> Result<ValidatedPlan, Error> {
        Err(Error::Unsupported)
    }
}
impl ValidatedPlan {
    pub fn review(&self) -> &[ReviewedMove] {
        &[]
    }
}
impl Engine {
    pub fn begin(_: &Env, _: &Path, _: ValidatedPlan) -> Result<Self, Error> {
        Err(Error::Unsupported)
    }
    pub fn open(_: &Env, _: &Path) -> Result<Self, Error> {
        Err(Error::Unsupported)
    }
    pub fn run(&mut self, _: &CancelToken) -> Result<Status, Error> {
        Err(Error::Unsupported)
    }
    pub fn reconcile(&mut self, _: &CancelToken) -> Result<Status, Error> {
        Err(Error::Unsupported)
    }
    pub fn undo(&mut self, _: &CancelToken) -> Result<Status, Error> {
        Err(Error::Unsupported)
    }
    pub fn status(&self) -> Status {
        Status::default()
    }
    pub fn durability_flushes(&self) -> u64 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_mutation_entry_point_refuses_before_touching_even_nonexistent_paths() {
        let env = Env::macos(Path::new("/definitely-not-a-fixture"), "none");
        let cancel = CancelToken::new();
        assert!(matches!(
            Inventory::capture(&env, &cancel),
            Err(Error::Unsupported)
        ));
        assert!(matches!(
            Engine::open(&env, Path::new("/must-not-be-created")),
            Err(Error::Unsupported)
        ));
        assert!(matches!(
            Engine::begin(
                &env,
                Path::new("/must-not-be-created"),
                ValidatedPlan { _private: () }
            ),
            Err(Error::Unsupported)
        ));
        let mut engine = Engine { _private: () };
        assert!(matches!(engine.run(&cancel), Err(Error::Unsupported)));
        assert!(matches!(engine.undo(&cancel), Err(Error::Unsupported)));
        assert!(matches!(engine.reconcile(&cancel), Err(Error::Unsupported)));
    }
}
