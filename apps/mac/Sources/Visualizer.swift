// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import OpenGL.GL3

/// libprojectM, the Milkdrop engine, when it is beside the app.
///
/// The library is built apart (`vis/build-projectm.sh`) and loaded when the Visualizer window
/// first opens: the app builds and runs without it. Only its C interface is used.
struct ProjectM {
    typealias Handle = OpaquePointer

    let create: @convention(c) () -> Handle?
    let destroy: @convention(c) (Handle?) -> Void
    let setWindowSize: @convention(c) (Handle?, Int, Int) -> Void
    let render: @convention(c) (Handle?) -> Void
    /// The samples, how many frames, and 1 for one channel or 2 for left and right in turn.
    let addSamples: @convention(c) (Handle?, UnsafePointer<Float>?, UInt32, Int32) -> Void
    /// The preset's file, and whether to blend into it.
    let loadPreset: @convention(c) (Handle?, UnsafePointer<CChar>?, Bool) -> Void
    let setFps: @convention(c) (Handle?, Int32) -> Void
    let setMeshSize: @convention(c) (Handle?, Int, Int) -> Void
    let setSoftCut: @convention(c) (Handle?, Double) -> Void
    let setPresetDuration: @convention(c) (Handle?, Double) -> Void

    /// The library from the app's Frameworks folder, or nil when it is not there.
    static let shared: ProjectM? = {
        guard let folder = Bundle.main.privateFrameworksPath,
            let library = dlopen(folder + "/libprojectM-4.dylib", RTLD_NOW | RTLD_LOCAL)
        else { return nil }
        func symbol<T>(_ name: String) -> T? {
            dlsym(library, name).map { unsafeBitCast($0, to: T.self) }
        }
        guard let create: @convention(c) () -> Handle? = symbol("projectm_create"),
            let destroy: @convention(c) (Handle?) -> Void = symbol("projectm_destroy"),
            let setWindowSize: @convention(c) (Handle?, Int, Int) -> Void = symbol("projectm_set_window_size"),
            let render: @convention(c) (Handle?) -> Void = symbol("projectm_opengl_render_frame"),
            let addSamples: @convention(c) (Handle?, UnsafePointer<Float>?, UInt32, Int32) -> Void = symbol(
                "projectm_pcm_add_float"),
            let loadPreset: @convention(c) (Handle?, UnsafePointer<CChar>?, Bool) -> Void = symbol(
                "projectm_load_preset_file"),
            let setFps: @convention(c) (Handle?, Int32) -> Void = symbol("projectm_set_fps"),
            let setMeshSize: @convention(c) (Handle?, Int, Int) -> Void = symbol("projectm_set_mesh_size"),
            let setSoftCut: @convention(c) (Handle?, Double) -> Void = symbol("projectm_set_soft_cut_duration"),
            let setPresetDuration: @convention(c) (Handle?, Double) -> Void = symbol("projectm_set_preset_duration")
        else { return nil }
        return ProjectM(
            create: create, destroy: destroy, setWindowSize: setWindowSize, render: render,
            addSamples: addSamples, loadPreset: loadPreset, setFps: setFps, setMeshSize: setMeshSize,
            setSoftCut: setSoftCut, setPresetDuration: setPresetDuration)
    }()
}

/// The view Milkdrop draws in: an OpenGL view at one pixel per point, which is what a preset
/// costs least at, fed with the samples the player heard since the last frame.
final class MilkdropView: NSOpenGLView {
    private let engine: ProjectM
    private let player: OpaquePointer
    private var handle: ProjectM.Handle?
    private var timer: Timer?
    private var samples = [Float](repeating: 0, count: 2048)
    /// The preset to load at the next frame, and whether to blend into it.
    private var wanted: (file: String, blend: Bool)?
    var onKey: ((NSEvent) -> Bool)?
    /// Frames drawn so far, and the processor time they took on this thread, for `--bench`.
    private(set) var framesDrawn = 0
    private(set) var frameSeconds = 0.0
    /// Frames a second, as the listener set them. A frame costs 3 to 4 ms of processor time
    /// on this thread at 640 by 400, most of it in the system's OpenGL.
    var fps: Double = 30 {
        didSet {
            guard fps != oldValue, let handle else { return }
            openGLContext?.makeCurrentContext()
            engine.setFps(handle, Int32(fps))
        }
    }

    init?(engine: ProjectM, player: OpaquePointer) {
        self.engine = engine
        self.player = player
        let attributes: [NSOpenGLPixelFormatAttribute] = [
            UInt32(NSOpenGLPFAOpenGLProfile), UInt32(NSOpenGLProfileVersion4_1Core),
            UInt32(NSOpenGLPFAColorSize), 24, UInt32(NSOpenGLPFAAlphaSize), 8,
            UInt32(NSOpenGLPFADoubleBuffer), UInt32(NSOpenGLPFAAccelerated), 0,
        ]
        guard let format = NSOpenGLPixelFormat(attributes: attributes) else { return nil }
        super.init(frame: NSRect(x: 0, y: 0, width: 640, height: 400), pixelFormat: format)
        // a point is one pixel: Milkdrop fills every pixel many times over
        wantsBestResolutionOpenGLSurface = false
    }

    required init?(coder: NSCoder) { fatalError("not from a nib") }

    deinit {
        timer?.invalidate()
        if let handle {
            openGLContext?.makeCurrentContext()
            engine.destroy(handle)
        }
    }

    override var acceptsFirstResponder: Bool { true }

    override func keyDown(with event: NSEvent) {
        if onKey?(event) != true { super.keyDown(with: event) }
    }

    override func prepareOpenGL() {
        super.prepareOpenGL()
        openGLContext?.makeCurrentContext()
        // a frame is handed over without waiting for the screen: the wait would hold up the
        // main thread, and the window server shows whole frames either way
        var sync: GLint = 0
        openGLContext?.setValues(&sync, for: .swapInterval)
        handle = engine.create()
        engine.setFps(handle, Int32(fps))
        engine.setMeshSize(handle, 48, 32)
        engine.setSoftCut(handle, 2)
        // the window changes presets itself
        engine.setPresetDuration(handle, 100_000)
        engine.setWindowSize(handle, Int(bounds.width), Int(bounds.height))
    }

    override func reshape() {
        super.reshape()
        guard let handle else { return }
        openGLContext?.makeCurrentContext()
        engine.setWindowSize(handle, max(1, Int(bounds.width)), max(1, Int(bounds.height)))
    }

    /// Shows the preset at `file`, blending into it or cutting to it.
    func load(preset file: String, blend: Bool) {
        wanted = (file, blend)
    }

    /// Draws frames while `running`, and none while the window is out of sight.
    func run(_ running: Bool) {
        timer?.invalidate()
        timer = nil
        guard running else { return }
        let next = Timer(timeInterval: 1.0 / fps, repeats: true) { [weak self] _ in self?.frame() }
        RunLoop.main.add(next, forMode: .common)
        timer = next
    }

    /// One frame: the samples heard since the last one go in, and a picture comes out.
    func frame() {
        guard let handle, let context = openGLContext else { return }
        let began = clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID)
        defer {
            framesDrawn += 1
            frameSeconds += Double(clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) - began) / 1e9
        }
        context.makeCurrentContext()
        if let wanted {
            engine.loadPreset(handle, wanted.file, wanted.blend)
            self.wanted = nil
        }
        let count = alpaca_player_take_wave(player, &samples, Int32(samples.count))
        if count > 0 { engine.addSamples(handle, samples, UInt32(count), 1) }
        engine.render(handle)
        context.flushBuffer()
    }

    /// The picture shown, for `--dump`.
    func picture() -> NSBitmapImageRep? {
        guard handle != nil, let context = openGLContext else { return nil }
        context.makeCurrentContext()
        let (width, height) = (Int(bounds.width), Int(bounds.height))
        guard width > 0, height > 0,
            let rep = NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 4,
                hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: width * 4, bitsPerPixel: 32),
            let data = rep.bitmapData
        else { return nil }
        // the frame just drawn is in the front buffer; OpenGL counts rows from the bottom
        glReadBuffer(GLenum(GL_FRONT))
        var rows = [UInt8](repeating: 0, count: width * height * 4)
        glReadPixels(0, 0, GLsizei(width), GLsizei(height), GLenum(GL_RGBA), GLenum(GL_UNSIGNED_BYTE), &rows)
        for row in 0..<height {
            let from = (height - 1 - row) * width * 4
            for i in 0..<(width * 4) { data[row * width * 4 + i] = i % 4 == 3 ? 255 : rows[from + i] }
        }
        return rep
    }
}

/// The visualizer: Milkdrop presets drawn to what plays, in the skin or in the Milkdrop window.
///
/// Which presets there are, which one shows, how long it stays, what its keys do and where it
/// shows is the library's, the same on every system. This is the view, the windows it shows in
/// and its timers. In the Milkdrop window the right and left arrows go on and back, R picks a
/// preset at random, and the space bar keeps the one shown.
///
/// In the skin it shows in a borderless window over the part of the skin window the library
/// names (`alpaca_app_vis_area`), a child of that window, so it moves along with it. It lets
/// the mouse through to the skin. The view is the same one the Milkdrop window shows: projectM
/// draws into a window's own framebuffer, and a view inside the skin's layers would have none.
final class Visualizer: NSObject, NSWindowDelegate {
    private let app: OpaquePointer
    private unowned let skin: SkinController
    /// The Milkdrop window.
    private var window: NSWindow?
    /// The window over the skin's visualizer area, and where in its skin window it is, in
    /// layout pixels from the top left corner.
    private var inset: NSWindow?
    private(set) var insetArea: (window: UInt32, rect: CGRect)?
    private var view: MilkdropView?
    private var changer: Timer?
    /// Called after the library was told something, for what it asks in answer.
    var onChange: (() -> Void)?

    init(app: OpaquePointer, skin: SkinController) {
        self.app = app
        self.skin = skin
        super.init()
    }

    /// Whether the engine is beside the app.
    static var available: Bool { ProjectM.shared != nil }

    var isShown: Bool { window?.isVisible == true }

    func close() { window?.close() }

    /// The view, made the first time it is needed; nil when the engine is not beside the app.
    private func madeView() -> MilkdropView? {
        if let view { return view }
        guard let engine = ProjectM.shared,
            let made = MilkdropView(engine: engine, player: alpaca_app_player(app))
        else { return nil }
        made.onKey = { [weak self] event in self?.key(event) ?? false }
        view = made
        return made
    }

    /// Opens the Milkdrop window, with the view in it.
    func show() {
        guard ProjectM.shared != nil, let view = madeView() else {
            let alert = NSAlert()
            alert.messageText = "The Milkdrop engine is not beside the app"
            alert.informativeText = "Build it with vis/build-projectm.sh, then build the app again."
            alert.runModal()
            return
        }
        letGoOfSkin()
        if window == nil {
            let made = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 640, height: 400),
                styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
            made.title = "Visualizer"
            made.isReleasedWhenClosed = false
            made.delegate = self
            made.contentMinSize = NSSize(width: 200, height: 120)
            made.setFrameAutosaveName("Visualizer")
            if made.frame.origin == .zero { made.center() }
            window = made
        }
        if window?.contentView !== view { window?.contentView = view }
        window?.makeKeyAndOrderFront(nil)
        window?.makeFirstResponder(view)
        run()
        // the library answers with the first preset of the listener's folder
        alpaca_app_vis_shown(app, true)
        onChange?()
    }

    /// Puts the view where the library says the skin shows the visualizer, or takes it out of
    /// the skin when it shows none there. Nothing changes while the Milkdrop window is open.
    func place() {
        guard window?.isVisible != true else { return }
        var id: UInt32 = 0
        var (x, y, w, h): (Int32, Int32, Int32, Int32) = (0, 0, 0, 0)
        guard alpaca_app_vis_area(app, &id, &x, &y, &w, &h), let host = skin.window(id), let view = madeView()
        else {
            letGoOfSkin()
            run()
            return
        }
        let z = CGFloat(skin.zoom)
        let frame = NSRect(
            x: host.frame.minX + CGFloat(x) * z, y: host.frame.maxY - CGFloat(y + h) * z,
            width: CGFloat(w) * z, height: CGFloat(h) * z)
        let over = inset ?? madeInset()
        if over.contentView !== view { over.contentView = view }
        if over.frame != frame { over.setFrame(frame, display: false) }
        if over.parent !== host {
            over.parent?.removeChildWindow(over)
            host.addChildWindow(over, ordered: .above)
        }
        over.level = host.level
        over.alphaValue = host.alphaValue
        over.orderFront(nil)
        insetArea = (id, CGRect(x: Int(x), y: Int(y), width: Int(w), height: Int(h)))
        run()
    }

    private func madeInset() -> NSWindow {
        let made = NSWindow(contentRect: .zero, styleMask: .borderless, backing: .buffered, defer: false)
        made.isOpaque = true
        made.hasShadow = false
        made.isReleasedWhenClosed = false
        made.animationBehavior = .none
        // a click there is the skin's: a drag moves its window, a double click asks for the
        // Milkdrop window
        made.ignoresMouseEvents = true
        made.delegate = self
        inset = made
        return made
    }

    /// The view is in the skin no more.
    private func letGoOfSkin() {
        guard let inset else { return }
        inset.parent?.removeChildWindow(inset)
        inset.orderOut(nil)
        insetArea = nil
    }

    /// Shows the preset the library asked for, with its name on the Milkdrop window.
    func load(preset file: String, title: String, blend: Bool) {
        madeView()?.load(preset: file, blend: blend)
        window?.title = title
    }

    private func key(_ event: NSEvent) -> Bool {
        let named: [UInt16: String] = [124: "right", 123: "left", 49: "space"]
        guard let key = named[event.keyCode] ?? event.charactersIgnoringModifiers?.lowercased(),
            alpaca_app_vis_key(app, key)
        else { return false }
        onChange?()
        return true
    }

    /// The window the view shows in now: the Milkdrop window, the one over the skin, or none.
    private var shownIn: NSWindow? {
        if window?.isVisible == true { return window }
        if insetArea != nil { return inset }
        return nil
    }

    /// Draws while the view can be seen, and tells the library when a preset's stay is over.
    private func run() {
        let seen = shownIn.map { $0.isVisible && $0.occlusionState.contains(.visible) } ?? false
        view?.fps = Double(alpaca_app_vis_fps(app))
        view?.run(seen)
        changer?.invalidate()
        changer = nil
        guard seen else { return }
        let next = Timer(timeInterval: TimeInterval(alpaca_app_vis_stay(app)), repeats: true) { [weak self] _ in
            guard let self else { return }
            alpaca_app_vis_tick(self.app)
            self.onChange?()
        }
        RunLoop.main.add(next, forMode: .common)
        changer = next
    }

    /// The listener changed how fast it draws or how long a preset stays.
    func settingsChanged() { run() }

    func windowDidChangeOcclusionState(_ notification: Notification) { run() }

    func windowWillClose(_ notification: Notification) {
        guard (notification.object as? NSWindow) === window else { return }
        view?.run(false)
        changer?.invalidate()
        changer = nil
        // the library asks for the view back in the skin when it shows it there
        alpaca_app_vis_shown(app, false)
        onChange?()
    }

    /// Frames drawn so far, and the processor time they took on the main thread.
    var frameCost: (frames: Int, seconds: Double) { (view?.framesDrawn ?? 0, view?.frameSeconds ?? 0) }

    /// Draws `frames` frames at once and gives the last as a picture, for `--dump`.
    func picture(after frames: Int) -> NSBitmapImageRep? {
        for _ in 0..<frames { view?.frame() }
        return view?.picture()
    }
}
