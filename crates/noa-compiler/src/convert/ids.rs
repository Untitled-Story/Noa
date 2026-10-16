use crate::diagnostics::Error;
use noa_format::ir::EntityId;
use std::collections::{BTreeMap, BTreeSet};

/// A normalized source identity (GUID when available), not an absolute path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceIdentity {
    pub asset: String,
    pub file_id: i64,
    pub binding: Option<(String, i64)>,
}

impl SourceIdentity {
    pub fn new(asset: impl Into<String>, file_id: i64) -> Self {
        Self {
            asset: asset.into(),
            file_id,
            binding: None,
        }
    }

    /// Clips and controller states are bound in the owning Animator's scope.
    /// Two Animator instances using the same source clip need different targets.
    pub fn bound_to(
        asset: impl Into<String>,
        file_id: i64,
        owner_asset: impl Into<String>,
        owner_id: i64,
    ) -> Self {
        Self {
            asset: asset.into(),
            file_id,
            binding: Some((owner_asset.into(), owner_id)),
        }
    }
}

/// Allocate once over the complete source set before converting any references.
/// Sorting makes assignment independent of traversal and insertion order.
#[derive(Debug)]
pub struct EntityIds(BTreeMap<SourceIdentity, EntityId>);

impl EntityIds {
    pub fn allocate(sources: impl IntoIterator<Item = SourceIdentity>) -> Result<Self, Error> {
        let mut ordered = BTreeSet::new();
        for source in sources {
            if source.file_id == 0
                || source.asset.is_empty()
                || source
                    .binding
                    .as_ref()
                    .is_some_and(|(asset, id)| asset.is_empty() || *id == 0)
            {
                return Err(Error::new("INVALID_SOURCE_ID", format!("{source:?}")));
            }
            if !ordered.insert(source.clone()) {
                return Err(Error::new("DUPLICATE_SOURCE_ID", format!("{source:?}")));
            }
        }
        let mut ids = BTreeMap::new();
        for (index, source) in ordered.into_iter().enumerate() {
            let value = u32::try_from(index)
                .ok()
                .and_then(|v| v.checked_add(1))
                .and_then(EntityId::new)
                .ok_or_else(|| Error::new("ID_SPACE_EXHAUSTED", "too many source objects"))?;
            ids.insert(source, value);
        }
        Ok(Self(ids))
    }

    pub fn next_id(&self) -> Result<u32, Error> {
        u32::try_from(self.0.len())
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| Error::new("ID_SPACE_EXHAUSTED", "too many source objects"))
    }

    pub fn get(&self, asset: &str, file_id: i64) -> Result<EntityId, Error> {
        self.resolve(&SourceIdentity::new(asset, file_id))
    }

    pub fn resolve(&self, source: &SourceIdentity) -> Result<EntityId, Error> {
        self.0.get(source).copied().ok_or_else(|| {
            Error::new(
                "UNALLOCATED_SOURCE_ID",
                format!("{source:?}: ID was not allocated"),
            )
        })
    }
}
