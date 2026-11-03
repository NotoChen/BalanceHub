//! Hook members are re-derived from complete private source content and the
//! selected native parent; a namespace string alone proves no ownership.
use super::*;
use crate::services::agent_cli::grok::environment::hooks as native;

fn slots(value: &Value, namespace: &str) -> BTreeMap<String, (String, usize, usize)> {
    let mut slots = BTreeMap::new();
    if let Some(root) = value.as_object() {
        native::walk_slots(root, true, |event, group, handler| {
            slots.insert(
                native::native_id(namespace, event, group, handler),
                (event.to_owned(), group, handler),
            );
            true
        });
    }
    slots
}

fn parent_raw<'p>(record: &'p ParentRecord<'_>) -> Option<&'p Value> {
    match &payload(record.asset)?.evidence {
        Evidence::Manifest(value) => Some(value),
        Evidence::Convention(Some(value)) => Some(value.as_ref()),
        _ => None,
    }
}

impl<'a, 's> Input<'a, 's> {
    fn hook_value<'p>(
        &self,
        key: &SourceKey,
        source: &AgentAssetSourceSpec,
        record: &'p ParentRecord<'a>,
        raw: &'p Value,
    ) -> Option<(&'p Value, bool)> {
        let colocated = *key == record.parent.source_key();
        if source.source_kind != AgentAssetSourceKind::File
            || (!colocated
                && !matches!(&key.kind, SourceKind::ComponentFile { roles, .. } if roles.hooks))
            || (colocated && parent_raw(record) != Some(raw))
        {
            return None;
        }
        let inline = colocated
            && matches!(key.kind, SourceKind::Manifest { .. })
            && record.descriptor.hooks_inline.is_some();
        if inline {
            Some((record.descriptor.hooks_inline.as_ref()?, true))
        } else {
            let expected = manifest::component_path(
                &source.allowed_root,
                record.descriptor.hooks_path.as_deref()?,
            )?;
            (key.relative_path()? == expected).then_some((raw, false))
        }
    }

    pub(super) fn hook_child(
        &self,
        asset: &ParsedAgentAsset,
        key: &SourceKey,
        source: &AgentAssetSourceSpec,
        record: &ParentRecord<'a>,
        raw: &Value,
        location: (&str, usize, usize),
    ) -> Option<bool> {
        let (value, inline) = self.hook_value(key, source, record, raw)?;
        let namespace = manifest::hook_namespace(&record.parent.namespace, &source.path, inline)?;
        let expected = slots(value, &namespace);
        let actual = expected.get(&asset.native_id)?;
        if actual.0 != location.0
            || actual.1 != location.1
            || actual.2 != location.2
            || (*key == record.parent.source_key()
                && payload(asset)?.revision != record.parent.revision)
            || asset.category != AgentAssetCategory::Hook
            || asset.declared_state != AgentAssetDeclaredState::Enabled
            || asset.declaration_key != asset.native_id
            || asset.resolution_group_key != asset.native_id
            || asset.label != format!("Grok Hook：{}", location.0)
            || asset.trust_state != parse::package_trust(key.scope(), self.context.trust_context)
            || asset.details
                != (AgentAssetDetails::Hook {
                    managed: false,
                    enabled: AgentAssetDeclaredState::Enabled,
                    rule_count: Some(1),
                })
        {
            return None;
        }
        Some(inline)
    }

    pub(super) fn incomplete_colocated_hooks(
        &self,
        parents: &BTreeMap<String, ParentRecord<'a>>,
    ) -> BTreeSet<String> {
        let mut incomplete = BTreeSet::new();
        for record in parents.values() {
            let mut actual = BTreeMap::<&str, Vec<&ParsedAgentAsset>>::new();
            for asset in self
                .by_source
                .get(record.asset.source_key.as_str())
                .into_iter()
                .flatten()
                .filter(|asset| asset.category == AgentAssetCategory::Hook)
            {
                actual.entry(&asset.native_id).or_default().push(asset);
            }
            let expected = (|| {
                let source = self.source(&record.asset.source_key)?;
                let key = record.parent.source_key();
                let (value, inline) = self.hook_value(&key, source, record, parent_raw(record)?)?;
                let namespace =
                    manifest::hook_namespace(&record.parent.namespace, &source.path, inline)?;
                Some(slots(value, &namespace))
            })()
            .unwrap_or_default();
            for native_id in expected.keys() {
                let complete = actual.remove(native_id.as_str()).is_some_and(|assets| {
                    assets.len() == 1 && self.child(assets[0], parents).is_some()
                });
                if !complete {
                    incomplete.insert(native_id.clone());
                }
            }
            incomplete.extend(actual.into_keys().map(str::to_owned));
        }
        incomplete
    }
}
