use super::{
    Draft, DroppedDraft, Error, Folder, InventoryEntry, Ledger, PlannedDirectory, ReviewedMove,
    Status,
};
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
    pub fn capture_folder(_: &Env, _: Folder, _: &CancelToken) -> Result<Self, Error> {
        Err(Error::Unsupported)
    }
    pub fn entries(&self) -> &[InventoryEntry] {
        &[]
    }
    pub fn truncated(&self) -> bool {
        false
    }
    pub fn later(&self) -> u64 {
        0
    }
    pub fn validate_pinned(&self, _: &[Draft], _: &CancelToken) -> Result<ValidatedPlan, Error> {
        Err(Error::Unsupported)
    }
    pub fn validate(&self, _: &[Draft], _: &CancelToken) -> Result<ValidatedPlan, Error> {
        Err(Error::Unsupported)
    }
}
impl ValidatedPlan {
    pub fn review(&self) -> &[ReviewedMove] {
        &[]
    }
    pub fn dropped(&self) -> &[DroppedDraft] {
        &[]
    }
    pub fn directories(&self) -> Vec<PlannedDirectory> {
        Vec::new()
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
    pub fn run_observed(
        &mut self,
        _: &CancelToken,
        _: &mut dyn FnMut(usize, usize),
    ) -> Result<Status, Error> {
        Err(Error::Unsupported)
    }
    pub fn undo_observed(
        &mut self,
        _: &CancelToken,
        _: &mut dyn FnMut(usize, usize),
    ) -> Result<Status, Error> {
        Err(Error::Unsupported)
    }
    pub fn ledger(&self) -> Ledger {
        Ledger::default()
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
            Inventory::capture_folder(&env, Folder::Downloads, &cancel),
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
        assert!(matches!(
            engine.run_observed(&cancel, &mut |_, _| panic!("never observes")),
            Err(Error::Unsupported)
        ));
        assert!(matches!(
            engine.undo_observed(&cancel, &mut |_, _| panic!("never observes")),
            Err(Error::Unsupported)
        ));
        assert!(matches!(engine.reconcile(&cancel), Err(Error::Unsupported)));
    }
}
