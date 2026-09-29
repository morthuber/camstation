#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct UiMode {
    kiosk: bool,
    expanded_camera: Option<u64>,
    saving: bool,
}

impl UiMode {
    pub(super) fn new(kiosk: bool) -> Self {
        Self {
            kiosk,
            ..Self::default()
        }
    }

    pub(super) fn kiosk(&self) -> bool {
        self.kiosk
    }

    pub(super) fn set_kiosk(&mut self, kiosk: bool) {
        self.kiosk = kiosk;
        self.expanded_camera = None;
    }

    pub(super) fn expanded_camera(&self) -> Option<u64> {
        self.expanded_camera
    }

    pub(super) fn toggle_expanded(&mut self, camera: u64) -> Option<u64> {
        self.expanded_camera = (self.expanded_camera != Some(camera)).then_some(camera);
        self.expanded_camera
    }

    pub(super) fn collapse_expanded(&mut self) -> bool {
        self.expanded_camera.take().is_some()
    }

    pub(super) fn saving(&self) -> bool {
        self.saving
    }

    pub(super) fn begin_save(&mut self) -> bool {
        if self.saving {
            false
        } else {
            self.saving = true;
            true
        }
    }

    pub(super) fn finish_save(&mut self) {
        self.saving = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kiosk_transition_always_collapses_expansion() {
        let mut mode = UiMode::default();
        assert_eq!(mode.toggle_expanded(7), Some(7));
        mode.set_kiosk(true);
        assert!(mode.kiosk());
        assert_eq!(mode.expanded_camera(), None);
    }

    #[test]
    fn expansion_toggles_and_switches_camera() {
        let mut mode = UiMode::default();
        assert_eq!(mode.toggle_expanded(1), Some(1));
        assert_eq!(mode.toggle_expanded(2), Some(2));
        assert_eq!(mode.toggle_expanded(2), None);
        assert!(!mode.collapse_expanded());
    }

    #[test]
    fn only_one_save_can_be_active() {
        let mut mode = UiMode::default();
        assert!(mode.begin_save());
        assert!(!mode.begin_save());
        mode.finish_save();
        assert!(mode.begin_save());
    }
}
