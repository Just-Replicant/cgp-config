//! [`ProjectSection`]: borrow a nested config section from a context that owns it.
//!
//! `#[cgp_getter]` implements its provider for [`WithProvider`](cgp_fork::prelude::WithProvider)
//! of a [`FieldGetter`](cgp_fork::prelude::FieldGetter). [`ProjectSection`] is that getter: it
//! walks a [`Path!`](cgp_fork::prelude::Path) of field names through [`HasField`].
//!
//! `Path!` is the fork's namespace path, so the path is written with `@`:
//!
//! ```
//! use cgp_fork::prelude::*;
//! use cgp_config_core::ProjectSection;
//!
//! #[derive(HasField)]
//! struct Http {
//!     port: u16,
//! }
//!
//! #[derive(HasField)]
//! struct App {
//!     http: Http,
//! }
//!
//! #[derive(HasField)]
//! struct Request {
//!     config: App,
//! }
//!
//! #[cgp_getter]
//! trait HasHttpConfig {
//!     fn http_config(&self) -> &Http;
//! }
//!
//! delegate_components! {
//!     Request {
//!         HttpConfigGetterComponent:
//!             WithProvider<ProjectSection<Path!(@config.http)>>,
//!     }
//! }
//!
//! let request = Request { config: App { http: Http { port: 8080 } } };
//! assert_eq!(request.http_config().port, 8080);
//! ```
//!
//! The context and every struct the path steps through derive [`HasField`](cgp_fork::prelude::HasField).
//! A field that is `Arc<App>` or `&App` still projects: `HasField` follows [`Deref`](core::ops::Deref).
//! The getter returns a reference borrowed from the context. It does not borrow through a
//! lock or an `ArcSwap`.

use core::marker::PhantomData;

use cgp_fork::core::field::traits::{FieldGetter, MapField};
use cgp_fork::prelude::{Nil, PathCons};

/// Projects a section by walking `Path` (`Path!(@config.http)`) through [`HasField`].
///
/// Wire it as `WithProvider<ProjectSection<Path!(@config.http)>>` on the getter component
/// `#[cgp_getter]` generated (`HttpConfigGetterComponent` for `HasHttpConfig`). See the
/// [module docs](self) for a complete context.
pub struct ProjectSection<Path>(PhantomData<fn() -> Path>);

impl<Context, OutTag, Head, Tail, Mid, Value> FieldGetter<Context, OutTag>
    for ProjectSection<PathCons<Head, Tail>>
where
    Head: 'static,
    Context: MapField<Head, Value = Mid>,
    ProjectSection<Tail>: FieldGetter<Mid, OutTag, Value = Value>,
{
    type Value = Value;

    fn get_field(context: &Context, tag: PhantomData<OutTag>) -> &Self::Value {
        context.map_field(PhantomData, |next| {
            <ProjectSection<Tail> as FieldGetter<Mid, OutTag>>::get_field(next, tag)
        })
    }
}

impl<Context, OutTag> FieldGetter<Context, OutTag> for ProjectSection<Nil> {
    type Value = Context;

    fn get_field(context: &Context, _tag: PhantomData<OutTag>) -> &Self::Value {
        context
    }
}

#[cfg(test)]
mod tests {
    use cgp_fork::prelude::*;

    use super::ProjectSection;

    #[derive(HasField)]
    struct Http {
        port: u16,
    }

    #[derive(HasField)]
    struct App {
        http: Http,
        name: &'static str,
    }

    #[derive(HasField)]
    struct Request {
        config: App,
    }

    #[test]
    fn walks_a_two_segment_path() {
        let request = Request {
            config: App {
                http: Http { port: 9 },
                name: "demo",
            },
        };
        let http = <ProjectSection<Path!(@config.http)> as FieldGetter<Request, ()>>::get_field(
            &request,
            PhantomData,
        );
        assert_eq!(http.port, 9);
        let name = <ProjectSection<Path!(@config.name)> as FieldGetter<Request, ()>>::get_field(
            &request,
            PhantomData,
        );
        assert_eq!(*name, "demo");
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn walks_through_arc() {
        use alloc::sync::Arc;

        #[derive(HasField)]
        struct Held {
            config: Arc<App>,
        }

        let held = Held {
            config: Arc::new(App {
                http: Http { port: 7 },
                name: "arc",
            }),
        };
        let http = <ProjectSection<Path!(@config.http)> as FieldGetter<Held, ()>>::get_field(
            &held,
            PhantomData,
        );
        assert_eq!(http.port, 7);
    }
}
