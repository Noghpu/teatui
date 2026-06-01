/// State of a value fetched in the background.
///
/// The four states are kept distinct so views can render exactly the right
/// thing in each case — in particular, `Loading` is the only state where a
/// loading indicator is appropriate. `Stale` shows the previous value;
/// `Unknown` should rarely be visible (it means we forgot to prefetch).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Cached<T> {
    #[default]
    Unknown,
    Loading,
    Ready(T),
    Stale {
        value: T,
        refreshing: bool,
    },
}

impl<T> Cached<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Cached::Ready(v) | Cached::Stale { value: v, .. } => Some(v),
            Cached::Unknown | Cached::Loading => None,
        }
    }

    pub fn value_mut(&mut self) -> Option<&mut T> {
        match self {
            Cached::Ready(v) | Cached::Stale { value: v, .. } => Some(v),
            Cached::Unknown | Cached::Loading => None,
        }
    }

    pub fn is_refreshing(&self) -> bool {
        matches!(
            self,
            Cached::Loading
                | Cached::Stale {
                    refreshing: true,
                    ..
                }
        )
    }

    /// Transition into a loading state. If there's an existing value, it is
    /// preserved as `Stale { refreshing: true }`; otherwise becomes `Loading`.
    pub fn mark_loading(&mut self) {
        let prev = std::mem::replace(self, Cached::Unknown);
        *self = match prev {
            Cached::Ready(value) | Cached::Stale { value, .. } => Cached::Stale {
                value,
                refreshing: true,
            },
            Cached::Unknown | Cached::Loading => Cached::Loading,
        };
    }

    pub fn set(&mut self, value: T) {
        *self = Cached::Ready(value);
    }

    pub fn mark_stale(&mut self) {
        let prev = std::mem::replace(self, Cached::Unknown);
        *self = match prev {
            Cached::Ready(value) | Cached::Stale { value, .. } => Cached::Stale {
                value,
                refreshing: false,
            },
            other => other,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_with_prior_value_preserves_it_as_stale() {
        let mut c: Cached<u32> = Cached::Ready(42);
        c.mark_loading();
        assert_eq!(
            c,
            Cached::Stale {
                value: 42,
                refreshing: true
            }
        );
        assert_eq!(c.value(), Some(&42));
        assert!(c.is_refreshing());
    }

    #[test]
    fn loading_with_no_prior_value_is_plain_loading() {
        let mut c: Cached<u32> = Cached::Unknown;
        c.mark_loading();
        assert_eq!(c, Cached::Loading);
        assert_eq!(c.value(), None);
    }

    #[test]
    fn set_replaces_any_prior_state() {
        let mut c: Cached<u32> = Cached::Loading;
        c.set(7);
        assert_eq!(c, Cached::Ready(7));
        assert!(!c.is_refreshing());
    }
}
