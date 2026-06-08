//! Port of src/world/itemToastStore.ts — the "you picked up X" toast stack.
//! Repeated pickups of the same item merge into one toast (bumped count +
//! refreshed clock); the stack caps at MAX_TOASTS, dropping the oldest.
//!
//! The TS global subscribe/notify fan-out is HUD-only → SKIPPED; ported as a
//! `ToastStack` struct so tests use a fresh instance. The TS `born` came from
//! `performance.now()`; here it's an explicit `now: f64` arg to `push` for
//! determinism (the HUD owns auto-dismiss timing off `born`).

#[derive(Debug, Clone, PartialEq)]
pub struct ItemToast {
    pub id: i64,
    pub item_id: String,
    pub count: i64,
    /// clock (ms) when last pushed/refreshed — HUD uses it to time the fade.
    pub born: f64,
}

/// Most toasts shown at once; older ones drop off the top.
pub const MAX_TOASTS: usize = 5;

#[derive(Debug, Clone)]
pub struct ToastStack {
    toasts: Vec<ItemToast>,
    next_id: i64,
}

impl Default for ToastStack {
    fn default() -> Self {
        Self { toasts: Vec::new(), next_id: 1 }
    }
}

impl ToastStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toasts(&self) -> &[ItemToast] {
        &self.toasts
    }

    /// Announce an acquired item at time `now`. Merges into an existing toast for
    /// the same item (count += count, born refreshed); else appends a new toast,
    /// trimming the oldest past MAX_TOASTS.
    pub fn push(&mut self, item_id: &str, count: i64, now: f64) {
        if let Some(existing) = self.toasts.iter_mut().find(|t| t.item_id == item_id) {
            existing.count += count;
            existing.born = now;
        } else {
            self.toasts.push(ItemToast {
                id: self.next_id,
                item_id: item_id.to_string(),
                count,
                born: now,
            });
            self.next_id += 1;
            if self.toasts.len() > MAX_TOASTS {
                let drop = self.toasts.len() - MAX_TOASTS;
                self.toasts.drain(0..drop);
            }
        }
    }

    /// Convenience: push a single pickup (count 1).
    pub fn push_one(&mut self, item_id: &str, now: f64) {
        self.push(item_id, 1, now);
    }

    pub fn remove(&mut self, id: i64) {
        self.toasts.retain(|t| t.id != id);
    }

    pub fn reset(&mut self) {
        self.toasts.clear();
    }
}

#[cfg(test)]
mod tests {
    // Port of src/world/itemToastStore.test.ts (the subscribe test is dropped
    // with the listener machinery). `now` is passed explicitly; the TS used
    // performance.now() but never asserts the value.
    use super::*;

    #[test]
    fn adds_a_toast_for_the_item_with_count_1() {
        let mut s = ToastStack::new();
        s.push_one("bread", 0.0);
        let t = s.toasts();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].item_id, "bread");
        assert_eq!(t[0].count, 1);
    }

    #[test]
    fn merges_repeat_pickup_into_one_toast_with_higher_count() {
        let mut s = ToastStack::new();
        s.push_one("apple", 0.0);
        s.push_one("apple", 0.0);
        s.push_one("apple", 0.0);
        let t = s.toasts();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].count, 3);
    }

    #[test]
    fn keeps_separate_items_as_separate_toasts() {
        let mut s = ToastStack::new();
        s.push_one("bread", 0.0);
        s.push_one("fur", 0.0);
        let ids: Vec<&str> = s.toasts().iter().map(|x| x.item_id.as_str()).collect();
        assert_eq!(ids, vec!["bread", "fur"]);
    }

    #[test]
    fn caps_the_stack_at_max_toasts_dropping_the_oldest() {
        let mut s = ToastStack::new();
        for i in 0..(MAX_TOASTS + 2) {
            s.push_one(&format!("item_{i}"), 0.0);
        }
        let t = s.toasts();
        assert_eq!(t.len(), MAX_TOASTS);
        assert_eq!(t[0].item_id, "item_2"); // first two dropped
    }

    #[test]
    fn remove_removes_the_toast_with_the_given_id() {
        let mut s = ToastStack::new();
        s.push_one("bread", 0.0);
        s.push_one("fur", 0.0);
        let id = s.toasts()[0].id;
        s.remove(id);
        let ids: Vec<&str> = s.toasts().iter().map(|x| x.item_id.as_str()).collect();
        assert_eq!(ids, vec!["fur"]);
    }
}
