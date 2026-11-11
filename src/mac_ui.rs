//! Native AppKit windows and actions. Capture/routing remain outside the UI.
use anyhow::{Context, Result};
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
    sel, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSApplicationTerminateReply, NSBackingStoreType,
    NSButton, NSColor, NSEventMask, NSFont, NSMenu, NSMenuItem, NSPopUpButton, NSTextField, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    ns_string, NSDate, NSDefaultRunLoopMode, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString,
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    io::{Read, Write},
    os::unix::net::UnixStream,
};

#[derive(Clone, Copy)]
pub enum Action {
    Show,
    About,
    Mute,
    Permission,
    Diagnostics,
    MenuBarLabels,
    Star,
    DismissSupport,
    License,
    Quit,
}
#[derive(Default)]
struct Ivars {
    pending: RefCell<Vec<Action>>,
}
define_class!(
    // NSObject has no additional subclassing requirements. All callbacks run
    // on AppKit's main thread and enqueue actions instead of touching audio.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self,_app:&NSApplication,_visible:bool)->bool {self.push(Action::Show);true}
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn keep_running(&self,_app:&NSApplication)->bool {false}
        #[unsafe(method(applicationShouldTerminate:))]
        fn terminate(&self,_app:&NSApplication)->NSApplicationTerminateReply {self.push(Action::Quit);NSApplicationTerminateReply::TerminateCancel}
    }
    impl Delegate {
        #[unsafe(method(showLanMic:))] fn show(&self,_sender:Option<&AnyObject>){self.push(Action::Show)}
        #[unsafe(method(aboutLanMic:))] fn about(&self,_sender:Option<&AnyObject>){self.push(Action::About)}
        #[unsafe(method(muteLanMic:))] fn mute(&self,_sender:Option<&AnyObject>){self.push(Action::Mute)}
        #[unsafe(method(permissionLanMic:))] fn permission(&self,_sender:Option<&AnyObject>){self.push(Action::Permission)}
        #[unsafe(method(diagnosticsLanMic:))] fn diagnostics(&self,_sender:Option<&AnyObject>){self.push(Action::Diagnostics)}
        #[unsafe(method(menuLabelsLanMic:))] fn labels(&self,_sender:Option<&AnyObject>){self.push(Action::MenuBarLabels)}
        #[unsafe(method(starLanMic:))] fn star(&self,_sender:Option<&AnyObject>){self.push(Action::Star)}
        #[unsafe(method(dismissSupportLanMic:))] fn dismiss_support(&self,_sender:Option<&AnyObject>){self.push(Action::DismissSupport)}
        #[unsafe(method(licenseLanMic:))] fn license(&self,_sender:Option<&AnyObject>){self.push(Action::License)}
        #[unsafe(method(quitLanMic:))] fn quit(&self,_sender:Option<&AnyObject>){self.push(Action::Quit)}
    }
);
impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![super(Self::alloc(mtm).set_ivars(Ivars::default())), init] }
    }
    fn push(&self, action: Action) {
        self.ivars().pending.borrow_mut().push(action)
    }
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
fn label(
    view: &NSView,
    text: &str,
    frame: NSRect,
    size: f64,
    bold: bool,
    secondary: bool,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    let font = if bold {
        NSFont::boldSystemFontOfSize(size)
    } else {
        NSFont::systemFontOfSize(size)
    };
    label.setFont(Some(&font));
    let color = if secondary {
        NSColor::secondaryLabelColor()
    } else {
        NSColor::labelColor()
    };
    label.setTextColor(Some(&color));
    label.setSelectable(true);
    view.addSubview(&label);
    label
}
fn button(
    view: &NSView,
    text: &str,
    frame: NSRect,
    selector: Sel,
    target: &Delegate,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(text),
            Some(target.as_ref()),
            Some(selector),
            mtm,
        )
    };
    button.setFrame(frame);
    view.addSubview(&button);
    button
}
fn make_window(title: &str, w: f64, h: f64, mtm: MainThreadMarker) -> Retained<NSWindow> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect(0., 0., w, h),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    window.center();
    window
}
fn menu_item(
    menu: &NSMenu,
    title: &str,
    key: &str,
    selector: Sel,
    target: Option<&Delegate>,
    mtm: MainThreadMarker,
) {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(selector),
            &NSString::from_str(key),
        )
    };
    if let Some(target) = target {
        unsafe { item.setTarget(Some(target.as_ref())) }
    };
    menu.addItem(&item);
}
pub struct Ui {
    app: Retained<NSApplication>,
    delegate: Retained<Delegate>,
    pub window: Retained<NSWindow>,
    about: Retained<NSWindow>,
    support: Retained<NSWindow>,
    heading: Retained<NSTextField>,
    detail: Retained<NSTextField>,
    input: Retained<NSTextField>,
    receiver: Retained<NSTextField>,
    lan_mouse: Retained<NSTextField>,
    access: Retained<NSTextField>,
    virtual_mic: Retained<NSTextField>,
    mute: Retained<NSButton>,
    permission: Retained<NSButton>,
    menu_labels: Retained<NSPopUpButton>,
}
impl Ui {
    pub fn new(
        app: Retained<NSApplication>,
        mtm: MainThreadMarker,
        style: crate::config::MenuBarLabels,
    ) -> Result<Self> {
        let delegate = Delegate::new(mtm);
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let main = NSMenu::new(mtm);
        let root = NSMenuItem::new(mtm);
        root.setTitle(ns_string!("Lan Mic"));
        let app_menu = NSMenu::new(mtm);
        root.setSubmenu(Some(&app_menu));
        main.addItem(&root);
        menu_item(
            &app_menu,
            "About Lan Mic",
            "",
            sel!(aboutLanMic:),
            Some(&delegate),
            mtm,
        );
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu_item(
            &app_menu,
            "Settings…",
            ",",
            sel!(showLanMic:),
            Some(&delegate),
            mtm,
        );
        menu_item(
            &app_menu,
            "Mute forwarding",
            "",
            sel!(muteLanMic:),
            Some(&delegate),
            mtm,
        );
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu_item(&app_menu, "Hide Lan Mic", "h", sel!(hide:), None, mtm);
        menu_item(
            &app_menu,
            "Quit Lan Mic",
            "q",
            sel!(quitLanMic:),
            Some(&delegate),
            mtm,
        );
        let window_root = NSMenuItem::new(mtm);
        window_root.setTitle(ns_string!("Window"));
        let window_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Window"));
        window_root.setSubmenu(Some(&window_menu));
        main.addItem(&window_root);
        menu_item(
            &window_menu,
            "Lan Mic Settings",
            "0",
            sel!(showLanMic:),
            Some(&delegate),
            mtm,
        );
        menu_item(
            &window_menu,
            "Minimize",
            "m",
            sel!(performMiniaturize:),
            None,
            mtm,
        );
        menu_item(
            &window_menu,
            "Close Window",
            "w",
            sel!(performClose:),
            None,
            mtm,
        );
        app.setMainMenu(Some(&main));
        app.setWindowsMenu(Some(&window_menu));
        let window = make_window("Lan Mic Settings", 560., 530., mtm);
        let view = window.contentView().context("Window content view")?;
        label(
            &view,
            "Lan Mic",
            rect(28., 456., 500., 38.),
            26.,
            true,
            false,
            mtm,
        );
        label(
            &view,
            "Your microphone follows your keyboard and mouse.",
            rect(28., 427., 500., 25.),
            13.,
            false,
            true,
            mtm,
        );
        let heading = label(
            &view,
            "Getting ready…",
            rect(28., 369., 500., 30.),
            18.,
            true,
            false,
            mtm,
        );
        let detail = label(
            &view,
            "Waiting for connection status.",
            rect(28., 322., 500., 42.),
            13.,
            false,
            true,
            mtm,
        );
        let mut rows = Vec::new();
        for (i, title) in [
            "Microphone",
            "Other computer",
            "LAN Mouse",
            "Microphone access",
            "Receive microphone",
        ]
        .iter()
        .enumerate()
        {
            let y = 294. - i as f64 * 27.;
            label(&view, title, rect(28., y, 146., 23.), 13., false, true, mtm);
            rows.push(label(
                &view,
                "Checking…",
                rect(185., y, 342., 23.),
                13.,
                false,
                false,
                mtm,
            ));
        }
        label(
            &view,
            "Menu bar labels",
            rect(28., 141., 146., 23.),
            13.,
            false,
            true,
            mtm,
        );
        let menu_labels = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(182., 136., 249., 30.),
            false,
        );
        menu_labels.addItemWithTitle(ns_string!("Computer names"));
        menu_labels.addItemWithTitle(ns_string!("Mac / Linux"));
        menu_labels.selectItemAtIndex(if style == crate::config::MenuBarLabels::ComputerNames {
            0
        } else {
            1
        });
        unsafe {
            menu_labels.setTarget(Some(delegate.as_ref()));
            menu_labels.setAction(Some(sel!(menuLabelsLanMic:)));
        }
        view.addSubview(&menu_labels);
        let mute = button(
            &view,
            "Mute forwarding",
            rect(25., 85., 180., 34.),
            sel!(muteLanMic:),
            &delegate,
            mtm,
        );
        let permission = button(
            &view,
            "Microphone settings…",
            rect(214., 85., 222., 34.),
            sel!(permissionLanMic:),
            &delegate,
            mtm,
        );
        button(
            &view,
            "About Lan Mic",
            rect(25., 40., 135., 30.),
            sel!(aboutLanMic:),
            &delegate,
            mtm,
        );
        button(
            &view,
            "Open diagnostics",
            rect(166., 40., 155., 30.),
            sel!(diagnosticsLanMic:),
            &delegate,
            mtm,
        );
        label(
            &view,
            "Closing this window keeps Lan Mic running in the menu bar.",
            rect(28., 12., 505., 18.),
            11.,
            false,
            true,
            mtm,
        );
        let about = make_window("About Lan Mic", 440., 350., mtm);
        let av = about.contentView().unwrap();
        label(
            &av,
            "Lan Mic",
            rect(28., 279., 385., 38.),
            26.,
            true,
            false,
            mtm,
        );
        label(
            &av,
            &format!("Version {} · Rust", env!("CARGO_PKG_VERSION")),
            rect(28., 249., 385., 25.),
            13.,
            false,
            true,
            mtm,
        );
        label(
            &av,
            &format!("Created by {}", env!("CARGO_PKG_AUTHORS")),
            rect(28., 218., 385., 25.),
            13.,
            false,
            false,
            mtm,
        );
        label(
            &av,
            "Your microphone, wherever you work.",
            rect(28., 175., 385., 27.),
            16.,
            true,
            false,
            mtm,
        );
        label(&av,"Automatically follows LAN Mouse between your computers. Audio travels directly over your authenticated SSH connection.",rect(28.,114.,385.,56.),13.,false,false,mtm);
        label(
            &av,
            "No cloud relay. No audio recordings.\nMicrophone access stays under your control.",
            rect(28., 59., 385., 47.),
            12.,
            false,
            true,
            mtm,
        );
        label(
            &av,
            "GPL-3.0-or-later",
            rect(28., 34., 132., 20.),
            11.,
            false,
            true,
            mtm,
        );
        button(
            &av,
            "License",
            rect(163., 23., 87., 32.),
            sel!(licenseLanMic:),
            &delegate,
            mtm,
        );
        label(
            &av,
            "Distributed without warranty",
            rect(28., 6., 385., 18.),
            10.,
            false,
            true,
            mtm,
        );
        button(
            &av,
            "Star on GitHub",
            rect(253., 23., 160., 32.),
            sel!(starLanMic:),
            &delegate,
            mtm,
        );
        let support = make_window("Support Lan Mic", 420., 200., mtm);
        let sv = support.contentView().context("Support window content")?;
        label(
            &sv,
            "Like LAN Mic? Star us on Github!",
            rect(24., 135., 374., 42.),
            19.,
            true,
            false,
            mtm,
        );
        label(
            &sv,
            "A star helps others find the project. Thanks for using Lan Mic.",
            rect(24., 78., 374., 45.),
            13.,
            false,
            true,
            mtm,
        );
        button(
            &sv,
            "Star on GitHub",
            rect(232., 22., 164., 34.),
            sel!(starLanMic:),
            &delegate,
            mtm,
        );
        button(
            &sv,
            "Don’t show again",
            rect(20., 22., 194., 34.),
            sel!(dismissSupportLanMic:),
            &delegate,
            mtm,
        );
        let mut rows = rows.into_iter();
        Ok(Self {
            app,
            delegate,
            window,
            about,
            support,
            heading,
            detail,
            input: rows.next().unwrap(),
            receiver: rows.next().unwrap(),
            lan_mouse: rows.next().unwrap(),
            access: rows.next().unwrap(),
            virtual_mic: rows.next().unwrap(),
            mute,
            permission,
            menu_labels,
        })
    }
    pub fn menu_bar_labels(&self) -> crate::config::MenuBarLabels {
        if self.menu_labels.indexOfSelectedItem() == 0 {
            crate::config::MenuBarLabels::ComputerNames
        } else {
            crate::config::MenuBarLabels::Platforms
        }
    }
    pub fn show(&self) {
        self.window.deminiaturize(None);
        self.window.makeKeyAndOrderFront(None);
        self.app.activate();
    }
    pub fn show_about(&self) {
        self.about.makeKeyAndOrderFront(None);
        self.app.activate();
    }
    pub fn support_prompt_ready(&self) -> bool {
        self.window.isVisible() && self.app.isActive() && !self.about.isVisible()
    }
    pub fn show_support(&self) {
        self.support.makeKeyAndOrderFront(None);
    }
    pub fn dismiss_support(&self) {
        self.support.orderOut(None);
    }
    pub fn poll(&self) -> Vec<Action> {
        objc2::rc::autoreleasepool(|_| {
            if let Some(event) = self.app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&NSDate::dateWithTimeIntervalSinceNow(0.01)),
                unsafe { NSDefaultRunLoopMode },
                true,
            ) {
                self.app.sendEvent(&event);
            }
            self.app.updateWindows();
        });
        self.delegate.ivars().pending.take()
    }
    pub fn update(&self, state: &Value) {
        let local = state["computer_name"].as_str().unwrap_or("This Mac");
        let peer = state["peer_name"].as_str().unwrap_or("Other computer");
        let input = state["input"].as_str().unwrap_or("System default");
        let connected = state["connected"] == true;
        let daemon = state["daemon"] == true;
        let authorized = state["authorized"] == true;
        let muted = state["muted"] == true;
        let capturing = state["capturing"] == true;
        let receiving = state["receiving"] == true;
        let reason = state["reason"].as_str().unwrap_or("Checking status");
        let title = if muted {
            "Forwarding is muted".into()
        } else if capturing {
            format!("{local} → {peer}")
        } else if receiving {
            format!("{peer} → {local}")
        } else {
            format!("Microphone on {local}")
        };
        self.heading.setStringValue(&NSString::from_str(&title));
        self.detail.setStringValue(&NSString::from_str(reason));
        let color = if capturing || receiving {
            NSColor::systemGreenColor()
        } else {
            NSColor::labelColor()
        };
        self.heading.setTextColor(Some(&color));
        self.input.setStringValue(&NSString::from_str(input));
        self.receiver.setStringValue(&NSString::from_str(&format!(
            "{peer} — {}",
            if connected {
                "connected"
            } else {
                "waiting for connection"
            }
        )));
        self.lan_mouse
            .setStringValue(&NSString::from_str(if daemon {
                "Running"
            } else {
                "Not running"
            }));
        self.access
            .setStringValue(&NSString::from_str(if authorized {
                "Allowed"
            } else {
                "Permission required"
            }));
        self.mute.setTitle(&NSString::from_str(if muted {
            "Unmute forwarding"
        } else {
            "Mute forwarding"
        }));
        self.permission.setEnabled(true);
        self.virtual_mic.setStringValue(&NSString::from_str(
            if state["virtual_microphone_available"] == true {
                "Ready"
            } else {
                "Needs BlackHole 2ch / restart"
            },
        ));
    }
    pub fn inspect(&self) -> Value {
        json!({"window_visible":self.window.isVisible(),"window_number":self.window.windowNumber(),"about_visible":self.about.isVisible(),"about_window_number":self.about.windowNumber(),"author":env!("CARGO_PKG_AUTHORS"),"title":self.window.title().to_string(),"heading":self.heading.stringValue().to_string(),"detail":self.detail.stringValue().to_string(),"mute_button":self.mute.title().to_string(),"input":self.input.stringValue().to_string(),"menu_bar_labels":if self.menu_labels.indexOfSelectedItem()==0{"computer_names"}else{"platforms"}})
    }
    pub fn preview(&self) -> Result<Value> {
        for (name, window) in [("window", &self.window), ("about", &self.about)] {
            let view = window.contentView().unwrap();
            window.displayIfNeeded();
            let data = view.dataWithPDFInsideRect(view.bounds());
            std::fs::write(
                crate::home().join(format!(
                    "Library/Application Support/Lan Mic/{name}-preview.pdf"
                )),
                unsafe { data.as_bytes_unchecked() },
            )?;
        }
        Ok(self.inspect())
    }
}
pub fn request(command: &str) -> Result<()> {
    let mut socket =
        UnixStream::connect(crate::home().join("Library/Application Support/Lan Mic/ui.sock"))
            .context("Lan Mic is not running; open the app first")?;
    socket.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
    socket.write_all(command.as_bytes())?;
    socket.shutdown(std::net::Shutdown::Write)?;
    let mut reply = String::new();
    socket.read_to_string(&mut reply)?;
    println!("{reply}");
    Ok(())
}
