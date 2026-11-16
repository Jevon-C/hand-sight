//! Page layout shared by every screen: skip link, header with navigation, and main area.

use leptos::prelude::*;
use leptos_router::components::A;

/// One entry in the main navigation.
struct NavItem {
    label: &'static str,
    href: &'static str,
}

const NAV: [NavItem; 3] = [
    NavItem {
        label: "Analysis",
        href: "/",
    },
    NavItem {
        label: "Comparison",
        href: "/compare",
    },
    NavItem {
        label: "Library",
        href: "/library",
    },
];

/// Side rail with the navigation, and the main region around the current screen.
#[component]
pub fn Shell(children: Children) -> impl IntoView {
    view! {
        <a class="skip-link" href="#main">"Skip to content"</a>
        <div class="app">
            <aside class="rail">
                <span class="brand">
                    <svg width="28" height="28" viewBox="0 0 28 28" aria-hidden="true" focusable="false">
                        <rect width="28" height="28" rx="7" fill="#9cc2ff"></rect>
                        <path
                            d="M9 7.5 V20.5 M19 7.5 V20.5 M9 14 H19"
                            fill="none"
                            stroke="#1b2733"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        ></path>
                    </svg>
                    <span class="brand__name">"Hand Sight"</span>
                </span>
                <nav aria-label="Main" class="nav">
                    {NAV
                        .iter()
                        .map(|item| view! { <A href=item.href>{item.label}</A> })
                        .collect_view()}
                </nav>
                <div class="rail__status">
                    <span class="rail__live"><span class="dot" aria-hidden="true"></span>"Running locally"</span>
                    <span>"Research prototype"</span>
                </div>
            </aside>
            <main id="main" class="main">{children()}</main>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::NAV;

    #[test]
    fn navigation_entries_are_distinct_absolute_paths() {
        let mut hrefs: Vec<_> = NAV.iter().map(|item| item.href).collect();
        assert!(hrefs.iter().all(|h| h.starts_with('/')));
        hrefs.sort_unstable();
        hrefs.dedup();
        assert_eq!(hrefs.len(), NAV.len());
        assert!(NAV.iter().all(|item| !item.label.is_empty()));
    }
}
