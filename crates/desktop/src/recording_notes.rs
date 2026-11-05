//! Notes drafts are keyed by recording, independently of inspection/comparison.
use igpu_core::model::Recording;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Drafts(BTreeMap<i64, (u64, String)>);
impl Drafts {
    pub(super) fn edit(&mut self, recording: &Recording) -> &mut String {
        let draft = self
            .0
            .entry(recording.id)
            .or_insert_with(|| (recording.started_ns, recording.notes.clone()));
        if draft.0 != recording.started_ns {
            *draft = (recording.started_ns, recording.notes.clone());
        }
        &mut draft.1
    }
    pub(super) fn payload(&self, id: i64) -> Option<Value> {
        self.0
            .get(&id)
            .map(|(_, notes)| json!({"id":id,"notes":notes}))
    }
    pub(super) fn retain(&mut self, recordings: &[Recording]) {
        self.0.retain(|id, (started, _)| {
            recordings
                .iter()
                .any(|r| r.id == *id && r.started_ns == *started)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn recording(id: i64, notes: &str) -> Recording {
        serde_json::from_value(json!({"id":id,"label":"fixture","started_ns":0,
            "finished_ns":1,"state":"complete","notes":notes,"metadata":{},"samples":0,"markers":[]})).unwrap()
    }
    #[test]
    fn switching_inspection_or_catalog_updates_cannot_cross_write_notes() {
        let a = recording(1, "saved A");
        let b = recording(2, "saved B");
        let mut drafts = Drafts::default();
        assert!(drafts.payload(1).is_none());
        *drafts.edit(&a) = "unsaved A".into();
        assert_eq!(drafts.edit(&b), "saved B");
        // Inspecting B and receiving catalog updates must preserve A's target/text.
        assert_eq!(
            drafts.payload(1).unwrap(),
            json!({"id":1,"notes":"unsaved A"})
        );
        assert_eq!(drafts.edit(&recording(1, "new catalog value")), "unsaved A");
        *drafts.edit(&b) = "unsaved B".into();
        assert_eq!(
            drafts.payload(2).unwrap(),
            json!({"id":2,"notes":"unsaved B"})
        );
        drafts.retain(&[a]);
        assert!(drafts.payload(2).is_none());
        assert_eq!(drafts.payload(1).unwrap()["notes"], "unsaved A");
        let mut replacement = recording(1, "different recording after deletion");
        replacement.started_ns = 100;
        assert_eq!(
            drafts.edit(&replacement),
            "different recording after deletion"
        );
        assert_eq!(drafts.payload(1).unwrap()["notes"], replacement.notes);
    }
}
