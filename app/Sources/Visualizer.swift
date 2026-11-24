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
    /// Frames a second. A frame costs about 5 ms of processor time whatever the preset, most of
    /// it in the system's OpenGL, so 30 it is until that is looked into; `ANDAMP_VIS_FPS` in the
    /// environment sets another number, for measuring.
    static let fps: Double = ProcessInfo.processInfo.environment["ANDAMP_VIS_FPS"].flatMap(Double.init) ?? 30

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
        engine.setFps(handle, Int32(Self.fps))
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
        let next = Timer(timeInterval: 1.0 / Self.fps, repeats: true) { [weak self] _ in self?.frame() }
        RunLoop.main.add(next, forMode: .common)
        timer = next
    }

    /// One frame: the samples heard since the last one go in, and a picture comes out.
    func frame() {
        guard let handle, let context = openGLContext else { return }
        context.makeCurrentContext()
        if let wanted {
            engine.loadPreset(handle, wanted.file, wanted.blend)
            self.wanted = nil
        }
        let count = andamp_player_take_wave(player, &samples, Int32(samples.count))
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

/// The Visualizer window: Milkdrop presets drawn to what plays.
///
/// Which presets there are, which one shows, how long it stays and what the window's keys do
/// is the library's, the same on every system. This is the window, its view and its timers.
/// The right and left arrows go on and back, R picks a preset at random, and the space bar
/// keeps the one shown.
final class Visualizer: NSObject, NSWindowDelegate {
    private let app: OpaquePointer
    private var window: NSWindow?
    private var view: MilkdropView?
    private var changer: Timer?
    /// Called after the library was told something, for what it asks in answer.
    var onChange: (() -> Void)?

    init(app: OpaquePointer) {
        self.app = app
        super.init()
    }

    /// Whether the engine is beside the app.
    static var available: Bool { ProjectM.shared != nil }

    var isShown: Bool { window?.isVisible == true }

    func close() { window?.close() }

    func show() {
        guard let engine = ProjectM.shared else {
            let alert = NSAlert()
            alert.messageText = "The Milkdrop engine is not beside the app"
            alert.informativeText = "Build it with vis/build-projectm.sh, then build the app again."
            alert.runModal()
            return
        }
        if window == nil {
            guard let view = MilkdropView(engine: engine, player: andamp_app_player(app)) else { return }
            let made = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 640, height: 400),
                styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
            made.title = "Visualizer"
            made.isReleasedWhenClosed = false
            made.delegate = self
            made.contentView = view
            made.contentMinSize = NSSize(width: 200, height: 120)
            made.setFrameAutosaveName("Visualizer")
            if made.frame.origin == .zero { made.center() }
            view.onKey = { [weak self] event in self?.key(event) ?? false }
            self.view = view
            window = made
        }
        window?.makeKeyAndOrderFront(nil)
        window?.makeFirstResponder(view)
        run()
        // the library answers with the first preset of the listener's folder
        andamp_app_vis_shown(app, true)
        onChange?()
    }

    /// Shows the preset the library asked for, with its name on the window.
    func load(preset file: String, title: String, blend: Bool) {
        view?.load(preset: file, blend: blend)
        window?.title = title
    }

    private func key(_ event: NSEvent) -> Bool {
        let named: [UInt16: String] = [124: "right", 123: "left", 49: "space"]
        guard let key = named[event.keyCode] ?? event.charactersIgnoringModifiers?.lowercased(),
            andamp_app_vis_key(app, key)
        else { return false }
        onChange?()
        return true
    }

    /// Draws while the window can be seen, and tells the library when a preset's stay is over.
    private func run() {
        let seen = window?.isVisible == true && window?.occlusionState.contains(.visible) == true
        view?.run(seen)
        changer?.invalidate()
        changer = nil
        guard seen else { return }
        let next = Timer(timeInterval: TimeInterval(andamp_app_vis_stay()), repeats: true) { [weak self] _ in
            guard let self else { return }
            andamp_app_vis_tick(self.app)
            self.onChange?()
        }
        RunLoop.main.add(next, forMode: .common)
        changer = next
    }

    func windowDidChangeOcclusionState(_ notification: Notification) { run() }

    func windowWillClose(_ notification: Notification) {
        view?.run(false)
        changer?.invalidate()
        changer = nil
        andamp_app_vis_shown(app, false)
        onChange?()
    }

    /// Draws `frames` frames at once and gives the last as a picture, for `--dump`.
    func picture(after frames: Int) -> NSBitmapImageRep? {
        for _ in 0..<frames { view?.frame() }
        return view?.picture()
    }
}
