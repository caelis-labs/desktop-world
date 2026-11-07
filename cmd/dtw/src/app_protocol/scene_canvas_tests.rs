use super::*;

#[test]
fn current_canvas_receiver_cannot_switch_to_sibling_process_instance() {
    let (server, catalog) = agent();
    let a = application();
    let mut b = a.clone();
    b.pid += 1;
    b.generation -= 1;
    catalog.entries.lock().unwrap().push(Application {
        app_id: APP.into(),
        app: b.clone(),
        name: "Sibling".into(),
    });
    let row = |app: &AppInstance, native_id, rect, rank| WindowInfo {
        target: WindowTarget {
            app: app.clone(),
            native_id,
            generation: 3,
        },
        title: format!("Instance {}", app.pid),
        visible: true,
        scene: WindowLayout::new(rect, rank),
    };
    {
        let mut image = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut image, 20, 10);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![255; 20 * 10 * 4])
                .unwrap();
        }
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.capture_png = Some(image);
        model.instance_inventories.insert(
            (a.pid, a.generation),
            test_inventory(vec![row(&a, 101, [0., 0., 20., 10.], 1)]),
        );
        model.instance_inventories.insert(
            (b.pid, b.generation),
            test_inventory(vec![row(&b, 201, [15., 0., 5., 5.], 0)]),
        );
    }
    let seen = exec(&server, "canvas_seen", "await app.capture();");
    assert!(!seen.is_error, "{}", seen.data);
    catalog
        .backend
        .0
        .model
        .lock()
        .unwrap()
        .instance_inventories
        .insert(
            (b.pid, b.generation),
            test_inventory(vec![
                row(&b, 202, [0., 0., 5., 5.], 0),
                row(&b, 201, [15., 0., 5., 5.], 1),
            ]),
        );
    catalog
        .backend
        .0
        .model
        .lock()
        .unwrap()
        .instance_inventories
        .insert(
            (a.pid, a.generation),
            test_inventory(vec![row(&a, 101, [0., 0., 20., 10.], 2)]),
        );
    let reread = exec(&server, "semantic_reread", "await app.look();");
    assert!(
        !reread.is_error && reread.image.is_none(),
        "{}",
        reread.data
    );
    // A semantic read and refreshing an unrelated point must not authorize a sibling at a later
    // point that originally belonged to the selected base instance.
    let reply = exec(
        &server,
        "canvas_sibling",
        "await app.click({x:25,y:5}); await app.click({x:5,y:5});",
    );
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["script"]["failure"]["code"], "stale_scene");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}
