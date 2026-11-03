//! The public authorization route remains opaque even when native identities
//! use a canonical spelling. Normalization must not weaken wrong-app admission.
use super::*;

struct RoutedCatalog {
    catalog: Catalog,
    returned_route: String,
    canonical_instance: AppInstance,
}
impl Factory for RoutedCatalog {
    type Provider = MockBackend;
    fn catalog(&self) -> Result<Vec<Application>, Error> {
        self.catalog.catalog()
    }
    fn connect(&self) -> Result<MockBackend, Error> {
        self.catalog.connect()
    }
    fn close_catalog(&self) -> Result<(), Error> {
        Ok(())
    }
    fn resolve(&self, _: &str) -> Result<ApplicationTarget, Error> {
        Ok(ApplicationTarget {
            app_id: self.returned_route.clone(),
            name: "Example".into(),
            instances: vec![self.canonical_instance.clone()],
        })
    }
}

#[test]
fn exact_public_route_accepts_canonical_native_identity_but_rejects_changed_route() {
    let canonical = r"c:\program files\example\example.exe";
    for requested in [
        r"C:\Program Files\Example\Example.EXE",
        "C:/Program Files/Example/Example.EXE",
    ] {
        for preserve_route in [true, false] {
            let (_, catalog) = server();
            let mut instance = application();
            instance.identity = AppIdentity(canonical.into());
            let server = Server::new(RoutedCatalog {
                catalog: catalog.clone(),
                returned_route: if preserve_route {
                    requested.into()
                } else {
                    canonical.into()
                },
                canonical_instance: instance.clone(),
            });
            let Started::Call(call) = server.begin(json!("route"), "exec", json!({
                "app":requested,"source":"await app.look(); await app.typeText('probe'); return 17;"
            })) else { panic!("not admitted") };
            let reply = server.run(&call);
            assert_eq!(reply.data["app"], requested);
            if preserve_route {
                assert_eq!(success(&reply), 17);
                assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
                assert_eq!(
                    catalog.backend.0.input_targets.lock().unwrap()[0].app,
                    instance
                );
                assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
            } else {
                assert!(reply.is_error);
                assert_eq!(reply.data["error"]["code"], "wrong_app");
                assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
                assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
            }
            server.close().unwrap();
        }
    }
}
