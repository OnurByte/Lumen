//! Account Center data for the Accounts page. The store is async (SQLx on Tokio), so each
//! call runs on a background thread with a private current-thread runtime.
use lumen_live::AccountId;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: AccountId,
    pub label: String,
    pub platform: &'static str,
    pub handle: String,
    pub group: Option<String>,
    pub enabled: bool,
    pub live: bool,
    pub authorized: bool,
    pub broadcast: bool,
    pub active: bool,
    /// Last saved connection state and event count from `lumen-live connect`.
    pub state: Option<String>,
    pub events: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Use(AccountId),
    Live(AccountId, bool),
    Broadcast(AccountId, bool),
    Remove(AccountId),
}

/// Apply `op` (if any) and return the refreshed account list. Blocking.
pub fn run(home: &std::path::Path, op: Option<Op>) -> Result<Vec<Row>, String> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let c = lumen_accounts::open_at(home).await?;
        let e = |e: lumen_accounts::StoreError| e.to_string();
        match op {
            Some(Op::Use(id)) => c.set_active(&id).await.map_err(e)?,
            Some(Op::Live(id, on)) => c.set_live_enabled(&id, on).await.map_err(e)?,
            Some(Op::Broadcast(id, on)) => c.set_broadcast_enabled(&id, on).await.map_err(e)?,
            Some(Op::Remove(id)) => c.remove(&id).await.map_err(e)?,
            None => {}
        }
        let active = c.active().await.map_err(e)?.map(|a| a.id);
        let mut rows = Vec::new();
        for a in c.list().await.map_err(e)? {
            let m = c.metrics(&a.id).await.map_err(e)?;
            rows.push(Row {
                state: m.as_ref().and_then(|m| m.pointer("/state/state")).and_then(|v| v.as_str()).map(str::to_owned),
                events: m.as_ref().and_then(|m| m.get("events_received")).and_then(|v| v.as_u64()),
                active: Some(&a.id) == active.as_ref(),
                platform: a.platform.as_str(),
                handle: a.handle,
                group: a.group,
                enabled: a.enabled,
                live: a.live_enabled,
                authorized: a.broadcast.authorized,
                broadcast: a.broadcast.enabled,
                label: a.label,
                id: a.id,
            });
        }
        Ok(rows)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_and_applies_ops_against_a_real_store() {
        let dir = std::env::temp_dir().join(format!("lumen-accounts-ui-{}", std::process::id()));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let id = rt.block_on(async {
            let c = lumen_accounts::open_at(&dir).await.unwrap();
            c.add(lumen_accounts::NewAccount { platform: lumen_live::Platform::KwaiGlobal, label: "kw".into(), handle: "k1".into(), group: Some("g".into()) }).await.unwrap().id
        });
        let rows = run(&dir, Some(Op::Use(id.clone()))).unwrap();
        assert!(rows[0].active && rows[0].live && !rows[0].authorized);
        assert!(run(&dir, Some(Op::Broadcast(id.clone(), true))).is_err(), "unauthorized accounts cannot broadcast");
        assert!(!run(&dir, Some(Op::Live(id.clone(), false))).unwrap()[0].live);
        assert!(run(&dir, Some(Op::Remove(id))).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
