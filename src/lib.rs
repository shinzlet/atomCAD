// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! This is the main application crate for atomCAD.  It contains the main
//! windowing event loop, implementations of user interface elements and
//! associated application logic, and the platform-specific code for
//! initializing the application and handling events.  It also contains a fair
//! amount of other functionality that has not yet been moved into separate
//! crates.
//!
//! atomCAD is implemented as a single-window application, with a 3D view
//! showing the molecular parts and aseemblies being edited, and an overlay of
//! various tool widgets optimized for multi-touch interfaces.  The 3D view is
//! implemented using the [wgpu] crate, and the overlay is implemented using
//! [rui].  Native APIs are used whenever possible for other required user
//! interface elements.
//!
//! As of this writing, the application is still in the early stages of
//! development, and is not yet usable for any practical purpose.  The
//! following features are currently implemented:
//!
//! * A basic 3D view, with a camera that can be controlled using the mouse
//!   and keyboard.
//!
//! * A basic menu bar, with a File menu that can be used to open PDB files.
//!
//! As is common with binary applications, the main entry point is in the
//! `main.rs` file, and the rest of the application is implemented in this
//! crate, so that it is accessible to integration tests.
//!
//! [wgpu]: https://crates.io/crates/wgpu
//! [rui]: https://crates.io/crates/rui

/// The API for controlling the camera in the 3D view, and having it respond
/// to user events.
pub mod camera;
/// A platform-independent abstraction over the windowing system's interface
/// for menus and menubars.  Used to setup the application menubar on startup.
pub mod menubar;

// This module is not public.  It is a common abstraction over the various
// platform-specific APIs.  For example, `platform::menubar` exposes an API
// for taking a platform-independent `menubar::Menu` and instantiating it in
// the windowing system and attaching it to either the window or application
// object, as required.
//
// The APIs exposed by this module are meant to be called from the rest of the
// `atomCAD` crate.
pub(crate) mod platform;
// This module contains the platform-specific native API code used by
// `platform`.  It is not intended to be used directly by any other code.  In
// the future it may be moved to be a private submodule of `platform`.
pub(crate) mod platform_impl;

/// The user-visible name of the application, used for window titles and such.
pub const APP_NAME: &str = "atomCAD";

use camera::ArcballCamera;
use common::InputEvent;
use render::{GlobalRenderResources, Interactions, RenderOptions, Renderer};
use scene::{
    feature::{Feature, PdbFeature},
    Assembly, Component, Molecule,
};

use std::rc::Rc;
use ultraviolet::{Mat4, Vec3};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::KeyCode,
    window::{Window, WindowBuilder},
};

fn make_pdb_demo_scene() -> Molecule {
    Molecule::from_feature(Feature::PdbFeature(PdbFeature {
        name: "Neon Pump".into(),
        contents: include_str!("../assets/neon_pump_imm.pdb").into(),
    }))
}

#[allow(dead_code)]
fn make_salt_demo_scene() -> Molecule {
    let mut molecule = Molecule::from_feature(Feature::RootAtom(periodic_table::Element::Sodium));

    molecule.push_feature(Feature::BondedAtom(scene::feature::BondedAtom {
        target: scene::ids::AtomSpecifier::new(0),
        element: periodic_table::Element::Chlorine,
    }));

    molecule.apply_all_features();
    molecule
}

async fn resume_renderer(
    window: &Window,
) -> (Renderer, Rc<GlobalRenderResources>, Assembly, Interactions) {
    let (renderer, gpu_resources) = Renderer::new(
        window,
        RenderOptions {
            fxaa: Some(()), // placeholder
            attempt_gpu_driven: true,
        },
    )
    .await;

    let molecule = make_salt_demo_scene();
    let molecule = serde_json::to_string(&molecule).unwrap();
    let molecule: Molecule = serde_json::from_str(&molecule).unwrap();

    let assembly = Assembly::from_components([Component::from_molecule(molecule, Mat4::default())]);
    let interactions = Interactions::default();

    (renderer, gpu_resources, assembly, interactions)
}

#[cfg_attr(feature = "cargo-clippy", allow(clippy::too_many_arguments))]
fn handle_event(
    event: Event<()>,
    control_flow: &mut ControlFlow,
    window: &mut Option<Window>,
    renderer: &mut Option<Renderer>,
    gpu_resources: &mut Option<Rc<GlobalRenderResources>>,
    world: &mut Option<Assembly>,
    interactions: &mut Option<Interactions>,
    cursor_pos: &PhysicalPosition<f64>,
) {
    match event {
        Event::NewEvents(StartCause::Init) => {
            // Will be called once when the event loop starts.
        }
        Event::WindowEvent {
            event: WindowEvent::Resized(new_size),
            ..
        } => {
            if let Some(renderer) = renderer {
                renderer.resize(new_size);
            }
        }
        Event::MainEventsCleared => {
            // The event queue is empty, so we can safely redraw the window.
            if window.is_some() {
                // Winit prevents sizing with CSS, so we have to set
                // the size manually when on web.
                #[cfg(target_arch = "wasm32")]
                (|| {
                    use winit::dpi::PhysicalSize;
                    log::error!("Resizing window");
                    let win = web_sys::window()?;
                    let width = win.inner_width().ok()?.as_f64()?;
                    let height = win.inner_height().ok()?.as_f64()?;
                    window.as_ref().map(|window| {
                        let scale_factor = window.scale_factor();
                        let new_size = PhysicalSize::new(
                            (width * scale_factor) as u32,
                            (height * scale_factor) as u32,
                        );
                        window.set_inner_size(new_size);
                        if let Some(renderer) = renderer {
                            renderer.resize(new_size);
                        }
                        Some(())
                    })
                })();
                if let Some(renderer) = renderer {
                    if let Some(world) = world {
                        if let Some(_interactions) = interactions {
                            if let Some(gpu_resources) = gpu_resources {
                                world.synchronize_buffers(gpu_resources);
                            }
                            let (atoms, transforms) = world.collect_atoms_and_transforms();
                            renderer.render(atoms, transforms);
                        }
                    }
                }
            }
        }
        Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } => {
            // The user has requested to close the window.
            // Drop the window to fire the `Destroyed` event.
            *window = None;
        }
        Event::WindowEvent {
            event: WindowEvent::Destroyed,
            ..
        } => {
            // The window has been destroyed, time to exit stage left.
            *control_flow = ControlFlow::ExitWithCode(0);
        }
        Event::LoopDestroyed => {
            // The event loop has been destroyed, so we can safely terminate
            // the application.  This is the very last event we will ever
            // receive, so we can safely perform final rites.
        }
        Event::WindowEvent { event, .. } => {
            if let Some(renderer) = renderer {
                match event {
                    WindowEvent::KeyboardInput { event: key, .. } => {
                        if key.physical_key == KeyCode::Space && key.state == ElementState::Released
                        {
                            if let Some(window) = window {
                                match renderer
                                    .camera()
                                    .get_ray_from(cursor_pos, &window.inner_size())
                                {
                                    Some((ray_origin, ray_direction)) => {
                                        world.as_mut().unwrap().walk_mut(|molecule, _| {
                                            if let Some(hit) =
                                                molecule.get_ray_hit(ray_origin, ray_direction)
                                            {
                                                println!("Atom {:?} clicked!", hit);
                                                // molecule.push_feature(AtomFeature {
                                                //     target: hit,
                                                //     element: periodic_table::Element::Carbon,
                                                // });
                                                // molecule.apply_all_features();
                                                // molecule.reupload_atoms(
                                                //     gpu_resources.as_ref().unwrap(),
                                                // );
                                            }
                                        });
                                    }
                                    None => {
                                        println!("failed to create ray!");
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        renderer.camera().update(InputEvent::Window(event));
                    }
                }
            }
        }
        Event::DeviceEvent { event, .. } => {
            if let Some(renderer) = renderer {
                renderer.camera().update(InputEvent::Device(event));
            }
        }
        _ => {
            // Unknown event; do nothing.
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run(event_loop: EventLoop<()>, mut window: Option<Window>) {
    // The event handling loop is terminated when the main window is closed.
    // We can trigger this by dropping the window, so we wrap it in the Option
    // type.  This is a bit of a hack, but it works.  We require that we are
    // called with a valid window, however.
    window.as_ref().expect("window should exist");

    // On mobile platforms the window is destroyed when the application is
    // suspended, so we need to be able to drop these resources and recreate
    // as necessary.
    let mut renderer: Option<Renderer> = None;
    let mut gpu_resources: Option<Rc<GlobalRenderResources>> = None;
    let mut world: Option<Assembly> = None;
    let mut interactions: Option<Interactions> = None;
    let mut cursor_pos: PhysicalPosition<f64> = Default::default();

    // Run the event loop.
    event_loop.run(move |event, _, control_flow| {
        // When we are done handling this event, suspend until the next event.
        *control_flow = ControlFlow::Wait;

        // Handle events.
        match event {
            Event::Resumed => {
                // Called on iOS or Android when the application is brought
                // into focus.  We must (re-)create the window and any GPU
                // resources, because they don't persist across application
                // suspensions.
                futures::executor::block_on(async {
                    let (mut r, g, w, i) = resume_renderer(window.as_ref().unwrap()).await;
                    r.set_camera(ArcballCamera::new(Vec3::zero(), 100.0, 1.0));
                    renderer = Some(r);
                    gpu_resources = Some(g);
                    world = Some(w);
                    interactions = Some(i);
                });
            }
            Event::Suspended => {
                // Called on iOS or Android when the application is sent to
                // the background.  We preemptively destroy the window and any
                // used GPU resources as the system might take them from us.
                interactions = None;
                world = None;
                gpu_resources = None;
                renderer = None;
                window = None;
            }
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                // The event system does not expose the cursor position on-demand. We track all
                // the mouse movement events to make this easier to access later.
                cursor_pos = position;
            }
            _ => {
                // Process all other events.
                handle_event(
                    event,
                    control_flow,
                    &mut window,
                    &mut renderer,
                    &mut gpu_resources,
                    &mut world,
                    &mut interactions,
                    &cursor_pos,
                );
            }
        }
    })
}

#[cfg(target_arch = "wasm32")]
async fn run(event_loop: EventLoop<()>, mut window: Option<Window>) {
    // The event handling loop is terminated when the main window is closed.
    // We can trigger this by dropping the window, so we wrap it in the Option
    // type.  This is a bit of a hack, but it works.  We require that we are
    // called with a valid window, however.
    window.as_ref().expect("window should exist");

    // These resources are *supposed* to be created after receiving the
    // Event::Resumed message within the event loop.  However on since async
    // support on wasm is wonky, we can't call `resume_renderer` from within
    // the event loop.  There seems to be no problem with calling it here and
    // then never dropping the resources on Event::Suspended.
    let (mut r, g, w, i) = resume_renderer(window.as_ref().unwrap()).await;
    r.set_camera(ArcballCamera::new(Vec3::zero(), 100.0, 1.0));
    let mut renderer = Some(r);
    let mut gpu_resources = Some(g);
    let mut world = Some(w);
    let mut interactions = Some(i);
    let mut cursor_pos: PhysicalPosition<f64> = Default::default();

    // Run the event loop.
    event_loop.run(move |event, _, control_flow| {
        // When we are done handling this event, suspend until the next event.
        *control_flow = ControlFlow::Wait;

        // Handle events.
        match event {
            // Ignore these messages (see above).
            Event::Resumed => {}
            Event::Suspended => {}

            // The event system does not expose the cursor position on-demand.
            // We track all the mouse movement events to make this easier to
            // access later.
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                cursor_pos = position;
            }

            // Process all other events.
            _ => {
                handle_event(
                    event,
                    control_flow,
                    &mut window,
                    &mut renderer,
                    &mut gpu_resources,
                    &mut world,
                    &mut interactions,
                    &cursor_pos,
                );
            }
        }
    })
}

pub fn start(event_loop: winit::event_loop::EventLoop<()>) {
    // Create the main window.
    let window = match WindowBuilder::new().with_title(APP_NAME).build(&event_loop) {
        Err(e) => {
            println!("Failed to create window: {}", e);
            std::process::exit(1);
        }
        Ok(window) => window,
    };

    // Add the menu bar to the window / application instance, using native
    // APIs.
    menubar::setup_menu_bar(&window);

    #[cfg(not(target_arch = "wasm32"))]
    {
        #[cfg(not(target_os = "android"))]
        {
            env_logger::init();
        }
        #[cfg(target_os = "android")]
        {
            android_logger::init_once(
                android_logger::Config::default().with_max_level(log::LevelFilter::Trace),
            );
        }
        run(event_loop, Some(window));
    }
    #[cfg(target_arch = "wasm32")]
    {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
        console_log::init().expect("could not initialize logger");
        // Winit prevents sizing with CSS, so we have to set
        // the size manually when on web.
        use winit::dpi::PhysicalSize;
        let width = web_sys::window()
            .and_then(|win| win.inner_width().ok())
            .and_then(|w| w.as_f64())
            .unwrap_or(800.0);
        let height = web_sys::window()
            .and_then(|win| win.inner_height().ok())
            .and_then(|h| h.as_f64())
            .unwrap_or(600.0);
        let scale_factor = window.scale_factor();
        window.set_inner_size(PhysicalSize::new(
            width * scale_factor,
            height * scale_factor,
        ));
        // On wasm, append the canvas to the document body
        use winit::platform::web::WindowExtWebSys;
        web_sys::window()
            .and_then(|win| win.document())
            .and_then(|doc| {
                let dst = doc.get_element_by_id("app-container")?;
                let canvas = web_sys::Element::from(window.canvas()?);
                dst.append_child(&canvas).ok()?;
                Some(())
            })
            .expect("Couldn't append canvas to document body.");
        wasm_bindgen_futures::spawn_local(run(event_loop, Some(window)));
    }
}

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: AndroidApp) {
    use winit::event_loop::EventLoopBuilder;
    use winit::platform::android::EventLoopBuilderExtAndroid;
    start(
        EventLoopBuilder::with_user_event()
            .with_android_app(app)
            .build(),
    )
}

// End of File
