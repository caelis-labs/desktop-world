// Disposable AppKit provider faults. Never injected into another application.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>

static NSString *profile;
static NSUInteger countCalls, rangeCalls, valueCalls;
static void unreadable(void) {
    [NSException raise:@"NSAccessibilityException" format:@"Owned AX compatibility fixture"];
}

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
@interface FaultApplication : NSApplication @end
@implementation FaultApplication
- (void)reportException:(NSException *)exception {
    fprintf(stderr, "Fixture AppKit exception: %s: %s\n", exception.name.UTF8String, exception.reason.UTF8String);
    [super reportException:exception];
}
- (id)accessibilityAttributeValue:(NSString *)attribute {
    if ([profile isEqual:@"no-windows"] &&
        ([attribute isEqual:NSAccessibilityWindowsAttribute] || [attribute isEqual:NSAccessibilityChildrenAttribute])) {
        unreadable();
    }
    return [super accessibilityAttributeValue:attribute];
}
@end

@interface FaultWindow : NSWindow @end
@implementation FaultWindow
- (NSArray *)fixtureChildren {
    NSMutableArray *children = [self.contentView.subviews mutableCopy];
    for (NSUInteger kind = NSWindowCloseButton; kind <= NSWindowZoomButton; kind++) {
        NSButton *button = [self standardWindowButton:kind];
        if (button) [children addObject:button];
    }
    return NSAccessibilityUnignoredChildren(children);
}
- (id)accessibilityAttributeValue:(NSString *)attribute {
    if ([attribute isEqual:NSAccessibilityChildrenAttribute]) {
        valueCalls++;
        if ([profile isEqual:@"opaque"]) unreadable();
        return [self fixtureChildren];
    }
    return [super accessibilityAttributeValue:attribute];
}
- (NSUInteger)accessibilityArrayAttributeCount:(NSString *)attribute {
    if ([attribute isEqual:NSAccessibilityChildrenAttribute]) {
        countCalls++;
        if ([profile isEqual:@"count-range"] || [profile isEqual:@"opaque"]) unreadable();
        return [self fixtureChildren].count;
    }
    return [super accessibilityArrayAttributeCount:attribute];
}
- (NSArray *)accessibilityArrayAttributeValues:(NSString *)attribute index:(NSUInteger)index maxCount:(NSUInteger)maximum {
    if ([attribute isEqual:NSAccessibilityChildrenAttribute]) {
        rangeCalls++;
        if (![profile isEqual:@"normal"]) unreadable();
        NSArray *all = [self fixtureChildren];
        if (index >= all.count) return @[];
        return [all subarrayWithRange:NSMakeRange(index, MIN(maximum, all.count - index))];
    }
    return [super accessibilityArrayAttributeValues:attribute index:index maxCount:maximum];
}
@end
#pragma clang diagnostic pop

@class Controller;
@interface Pad : NSView
@property(nonatomic, weak) Controller *controller;
@end
@interface Controller : NSObject <NSApplicationDelegate>
@property(nonatomic) NSWindow *window;
@property(nonatomic) NSButton *button;
@property(nonatomic) NSTextField *field;
@property(nonatomic) Pad *pad;
@property(nonatomic) NSMutableArray *gestures;
@property(nonatomic) NSUInteger clicks, keys, wheels;
@property(nonatomic) NSMutableArray *mouse;
@property(nonatomic) NSMutableArray *pointerSamples;
@property(nonatomic) NSURL *root;
@property(nonatomic) id activity;
- (void)write;
- (NSArray *)pointer;
@end
@implementation Pad
- (BOOL)acceptsFirstMouse:(NSEvent *)event { return YES; }
- (void)updateTrackingAreas {
    for (NSTrackingArea *area in self.trackingAreas) [self removeTrackingArea:area];
    [self addTrackingArea:[[NSTrackingArea alloc] initWithRect:NSZeroRect
        options:NSTrackingActiveAlways | NSTrackingInVisibleRect | NSTrackingMouseMoved
        owner:self userInfo:nil]];
    [super updateTrackingAreas];
}
- (void)record:(NSString *)kind {
    [self.controller.gestures addObject:kind];
    [self.controller.pointerSamples addObject:@{@"kind":kind, @"position":[self.controller pointer]}];
    [self.controller write];
}
- (void)mouseMoved:(NSEvent *)event { [self record:@"move"]; }
- (void)mouseDown:(NSEvent *)event { [self record:@"down"]; }
- (void)mouseDragged:(NSEvent *)event { [self record:@"drag"]; }
- (void)mouseUp:(NSEvent *)event { [self record:@"up"]; }
- (void)rightMouseDown:(NSEvent *)event { [self record:@"right-down"]; }
- (void)rightMouseUp:(NSEvent *)event { [self record:@"right-up"]; }
- (void)drawRect:(NSRect)rect {
    [NSGraphicsContext saveGraphicsState];
    NSRectClip(self.bounds);
    [[NSColor systemTealColor] setFill]; NSRectFill(self.bounds);
    [NSGraphicsContext restoreGraphicsState];
}
@end

@implementation Controller
- (void)clicked:(id)sender { self.clicks++; [self write]; }
- (void)keyed:(id)sender { self.keys++; [self write]; }
- (void)quit:(id)sender { [NSApp terminate:nil]; }
- (NSArray *)pointer {
    NSPoint point = NSEvent.mouseLocation;
    return @[@(point.x), @(NSScreen.screens.firstObject.frame.size.height - point.y)];
}
- (NSArray *)point:(NSView *)view x:(CGFloat)x y:(CGFloat)y {
    NSPoint point = [self.window convertPointToScreen:[view convertPoint:NSMakePoint(x,y) toView:nil]];
    return @[@(point.x), @(NSScreen.screens.firstObject.frame.size.height - point.y)];
}
- (void)write {
    if (!self.window) return;
    NSString *text = self.field.currentEditor.string ?: self.field.stringValue ?: @"";
    NSRect frame = self.window.frame;
    NSDictionary *state = @{
        @"pid": @(NSProcessInfo.processInfo.processIdentifier), @"profile": profile,
        @"window": @(self.window.windowNumber), @"clicks": @(self.clicks), @"keys": @(self.keys),
        @"wheels": @(self.wheels), @"text": text, @"gestures": self.gestures,
        @"active": @(NSApp.active), @"key_window": @(self.window.keyWindow), @"mouse": self.mouse,
        @"frontmost_pid": @(NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier),
        @"pointer": [self pointer], @"pointer_samples": self.pointerSamples,
        @"pad_frame": NSStringFromRect(self.pad.frame), @"pad_bounds": NSStringFromRect(self.pad.bounds),
        @"pressed_mouse_buttons": @(NSEvent.pressedMouseButtons),
        @"bounds": @[@(frame.origin.x), @(NSScreen.screens.firstObject.frame.size.height - NSMaxY(frame)), @(frame.size.width), @(frame.size.height)],
        @"points": @{@"button": [self point:self.button x:80 y:18], @"field": [self point:self.field x:100 y:15],
                     @"pad-start": [self point:self.pad x:40 y:35], @"pad-end": [self point:self.pad x:340 y:35]},
        @"ax_calls": @{@"count": @(countCalls), @"range": @(rangeCalls), @"value": @(valueCalls)}
    };
    NSData *data = [NSJSONSerialization dataWithJSONObject:state options:0 error:nil];
    [data writeToURL:[self.root URLByAppendingPathComponent:@"state.json"] atomically:YES];
}
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    self.root = NSBundle.mainBundle.bundleURL.URLByDeletingLastPathComponent;
    self.activity = [NSProcessInfo.processInfo beginActivityWithOptions:NSActivityUserInitiatedAllowingIdleSystemSleep
        reason:@"Owned test oracle must remain responsive while its window is occluded"];
    profile = [[NSString stringWithContentsOfURL:[self.root URLByAppendingPathComponent:@"profile"]
        encoding:NSUTF8StringEncoding error:nil] stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    self.gestures = [NSMutableArray array];
    self.mouse = [NSMutableArray array];
    self.pointerSamples = [NSMutableArray array];
    self.window = [[FaultWindow alloc] initWithContentRect:NSMakeRect(160,160,480,340)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable backing:NSBackingStoreBuffered defer:NO];
    self.window.title = @"DTW AX compatibility";
    self.window.releasedWhenClosed = NO;
    self.window.acceptsMouseMovedEvents = YES;
    if ([profile isEqual:@"occluder"]) {
        self.window.level = NSFloatingWindowLevel;
        self.window.hidesOnDeactivate = NO;
    }
    self.button = [NSButton buttonWithTitle:@"Next Tip" target:self action:@selector(clicked:)];
    self.button.frame = NSMakeRect(30,180,160,36);
    [self.window.contentView addSubview:self.button];
    self.field = [[NSTextField alloc] initWithFrame:NSMakeRect(30,255,380,30)];
    self.field.stringValue = @"initial";
    [self.window.contentView addSubview:self.field];
    self.pad = [[Pad alloc] initWithFrame:NSMakeRect(30,35,400,90)];
    self.pad.controller = self;
    [self.window.contentView addSubview:self.pad];
    NSMenu *main = [[NSMenu alloc] init];
    NSMenuItem *appItem = [[NSMenuItem alloc] init]; [main addItem:appItem];
    NSMenu *appMenu = [[NSMenu alloc] init]; appItem.submenu = appMenu;
    NSMenuItem *key = [appMenu addItemWithTitle:@"Owned Shortcut" action:@selector(keyed:) keyEquivalent:@"k"]; key.target = self;
    NSMenuItem *quit = [appMenu addItemWithTitle:@"Quit" action:@selector(quit:) keyEquivalent:@"q"]; quit.target = self;
    NSMenuItem *editItem = [[NSMenuItem alloc] init]; [main addItem:editItem];
    NSMenu *edit = [[NSMenu alloc] initWithTitle:@"Edit"]; editItem.submenu = edit;
    [edit addItemWithTitle:@"Select All" action:@selector(selectAll:) keyEquivalent:@"a"];
    NSApp.mainMenu = main;
    [NSEvent addLocalMonitorForEventsMatchingMask:NSEventMaskScrollWheel handler:^NSEvent *(NSEvent *event) {
        self.wheels++; [self write]; return event;
    }];
    [NSEvent addLocalMonitorForEventsMatchingMask:NSEventMaskLeftMouseDown | NSEventMaskLeftMouseUp handler:^NSEvent *(NSEvent *event) {
        [self.mouse addObject:@{@"kind": @(event.type), @"window": @(event.windowNumber),
                               @"x": @(event.locationInWindow.x), @"y": @(event.locationInWindow.y)}];
        [self write]; return event;
    }];
    [self.window makeKeyAndOrderFront:nil]; [NSApp activateIgnoringOtherApps:YES];
    NSTimer *timer = [NSTimer timerWithTimeInterval:0.05 repeats:YES block:^(NSTimer *timer) {
        if ([NSFileManager.defaultManager fileExistsAtPath:[self.root URLByAppendingPathComponent:@"quit"].path]) [NSApp terminate:nil];
        NSURL *move = [self.root URLByAppendingPathComponent:@"move"];
        if ([NSFileManager.defaultManager fileExistsAtPath:move.path]) {
            [NSFileManager.defaultManager removeItemAtURL:move error:nil];
            NSPoint origin = self.window.frame.origin; origin.x += 40;
            [self.window setFrameOrigin:origin];
        }
        [self write];
    }];
    [NSRunLoop.mainRunLoop addTimer:timer forMode:NSRunLoopCommonModes];
    [self write];
}
@end

int main(void) {
    @autoreleasepool {
        FaultApplication *app = [FaultApplication sharedApplication];
        Controller *controller = [[Controller alloc] init];
        app.delegate = controller;
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        [app run];
    }
    return 0;
}
