const serviceName = "io.github.terrydaktal.Screenshot";
const objectPath = "/io/github/terrydaktal/Screenshot";
const interfaceName = "io.github.terrydaktal.Screenshot";
const screenshotAppId = "io.github.terrydaktal.screenshot";
function isScreenshotWindow(window) {
    return window.resourceClass === screenshotAppId
        || window.resourceName === screenshotAppId;
}

function configureScreenshotWindow(window) {
    if (!isScreenshotWindow(window)) {
        return;
    }

    window.skipsCloseAnimation = readConfig("disableAnimations", true);
    window.noBorder = true;
    window.keepAbove = true;
    window.skipTaskbar = true;
    window.skipPager = true;
    window.skipSwitcher = true;
    window.onAllDesktops = true;
    window.frameGeometry = workspace.virtualScreenGeometry;
    workspace.activeWindow = window;
}

function watchWindow(window) {
    configureScreenshotWindow(window);
    window.windowClassChanged.connect(function () {
        configureScreenshotWindow(window);
    });
    window.captionChanged.connect(function () {
        configureScreenshotWindow(window);
    });
}

workspace.windowAdded.connect(watchWindow);
workspace.stackingOrder.forEach(watchWindow);

const shortcutRegistered = registerShortcut(
    "ScreenshotWaylandCapture",
    "Capture with screenshot",
    "Print",
    function () {
        callDBus(serviceName, objectPath, interfaceName, "Trigger");
    }
);

if (!shortcutRegistered) {
    console.warn("screenshot-wayland-trigger: failed to register the Print shortcut");
}
