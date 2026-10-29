//! Main-window pages in rail order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Today,
    History,
    Targets,
    Settings,
}
impl Page {
    pub const ALL: [Page; 4] = [Page::Today, Page::History, Page::Targets, Page::Settings];
    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::History => "History",
            Self::Targets => "Targets",
            Self::Settings => "Settings",
        }
    }
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap()
    }
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_round_trip_through_their_index() {
        for (i, page) in Page::ALL.iter().enumerate() {
            assert_eq!(page.index(), i);
            assert_eq!(Page::from_index(i), Some(*page));
            assert!(!page.label().is_empty());
        }
        assert_eq!(Page::from_index(4), None);
        assert_eq!(Page::default(), Page::Today);
    }
}
