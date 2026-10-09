//go:build darwin && cgo

#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>

@interface DWCursorView : NSView
@end
@implementation DWCursorView
- (BOOL)isOpaque { return NO; }
- (void)drawRect:(NSRect)dirty {
  (void)dirty;
  [[NSColor clearColor] set]; NSRectFill(self.bounds);
  NSBezierPath *outline = [NSBezierPath bezierPath];
  [outline moveToPoint:NSMakePoint(3, 26)];
  [outline lineToPoint:NSMakePoint(3, 3)];
  [outline lineToPoint:NSMakePoint(8, 8)];
  [outline lineToPoint:NSMakePoint(12, 1)];
  [outline lineToPoint:NSMakePoint(16, 3)];
  [outline lineToPoint:NSMakePoint(12, 10)];
  [outline lineToPoint:NSMakePoint(19, 10)];
  [outline closePath];
  [[NSColor colorWithSRGBRed:.10 green:.47 blue:.86 alpha:.94] setFill];
  [outline fill];
  [[NSColor whiteColor] setStroke];
  outline.lineWidth = 2.0;
  [outline stroke];
}
@end

static NSPanel *cursorWindow;

int dw_cursor_init(void) {
  @autoreleasepool {
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyProhibited];
    cursorWindow = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, 24, 30)
      styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
      backing:NSBackingStoreBuffered defer:NO];
    if (!cursorWindow) return 0;
    cursorWindow.opaque = NO;
    cursorWindow.backgroundColor = NSColor.clearColor;
    cursorWindow.hasShadow = NO;
    cursorWindow.ignoresMouseEvents = YES;
    cursorWindow.level = NSFloatingWindowLevel + 1;
    cursorWindow.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    cursorWindow.contentView = [[DWCursorView alloc] initWithFrame:NSMakeRect(0, 0, 24, 30)];
    return 1;
  }
}

int dw_cursor_show(double x, double y) {
  @autoreleasepool {
    if (!cursorWindow || !isfinite(x) || !isfinite(y)) return 0;
    for (NSScreen *screen in NSScreen.screens) {
      NSNumber *number = screen.deviceDescription[@"NSScreenNumber"];
      if (!number) continue;
      CGRect cg = CGDisplayBounds((CGDirectDisplayID)number.unsignedIntValue);
      if (x < CGRectGetMinX(cg) || x >= CGRectGetMaxX(cg) || y < CGRectGetMinY(cg) || y >= CGRectGetMaxY(cg)) continue;
      // Desktop World points use CoreGraphics top-left display coordinates.
      // Convert within the matching display; global NSScreen coordinates can
      // have a different origin and scale on secondary displays.
      CGFloat localX = (x - CGRectGetMinX(cg)) * screen.frame.size.width / cg.size.width;
      CGFloat localY = (y - CGRectGetMinY(cg)) * screen.frame.size.height / cg.size.height;
      NSPoint origin = NSMakePoint(NSMinX(screen.frame) + localX - 3,
                                   NSMaxY(screen.frame) - localY - 26);
      [cursorWindow setFrameOrigin:origin];
      [cursorWindow orderFrontRegardless];
      return 1;
    }
    [cursorWindow orderOut:nil];
    return 0;
  }
}

void dw_cursor_hide(void) { @autoreleasepool { [cursorWindow orderOut:nil]; } }
void dw_cursor_poll(void) { @autoreleasepool { [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:.001]]; } }
void dw_cursor_close(void) { @autoreleasepool { [cursorWindow orderOut:nil]; [cursorWindow close]; cursorWindow = nil; } }
