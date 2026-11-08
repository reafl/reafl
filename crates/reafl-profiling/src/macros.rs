//! Scope macros.
//!
//! Both macros take a `&'static str` name (a string literal or a `const`), so
//! recording never allocates and the flame graph shows the real symbol.
//! Prefer [`profile_span!`] when two scopes live in one block, because a second
//! `let` in the same block shadows the first guard and ends its span early.

/// Opens a scope that lasts until the end of the enclosing block.
///
/// ```ignore
/// fn render_mixer(&mut self) {
///     profile_scope!("render_mixer");
///     // ...
/// }
/// ```
#[macro_export]
macro_rules! profile_scope {
    ($name:expr) => {
        let _reafl_scope = $crate::span::Scope::enter($name);
    };
}

/// Creates a named guard: the scope ends when the binding is dropped.
///
/// ```ignore
/// let _load = profile_span!("load_plugin");
/// ```
#[macro_export]
macro_rules! profile_span {
    ($name:expr) => {
        $crate::span::Scope::enter($name)
    };
}
