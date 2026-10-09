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
  [outline moveToPoint:NSMakePoint(2, 20)];
  [outline lineToPoint:NSMakePoint(2, 2)];
  [outline lineToPoint:NSMakePoint(6.8, 7)];
  [outline lineToPoint:NSMakePoint(9.5, 1.8)];
  [outline lineToPoint:NSMakePoint(12.4, 3.1)];
  [outline lineToPoint:NSMakePoint(9.3, 8.7)];
  [outline lineToPoint:NSMakePoint(15.6, 8.9)];
  [outline closePath];
  NSGradient *tint = [[NSGradient alloc] initWithColorsAndLocations:
    [NSColor colorWithSRGBRed:.98 green:.76 blue:.85 alpha:.96], 0.0,
    [NSColor colorWithSRGBRed:.78 green:.77 blue:.99 alpha:.96], .54,
    [NSColor colorWithSRGBRed:.68 green:.91 blue:.88 alpha:.96], 1.0, nil];
  [tint drawInBezierPath:outline angle:-35];
  [[NSColor colorWithSRGBRed:.15 green:.20 blue:.29 alpha:.88] setStroke];
  outline.lineWidth = 1.15;
  outline.lineJoinStyle = NSLineJoinStyleRound;
  [outline stroke];
}
@end

static NSPanel *cursorWindow;

int dw_cursor_init(void) {
  @autoreleasepool {
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyProhibited];
    cursorWindow = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, 18, 22)
      styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
      backing:NSBackingStoreBuffered defer:NO];
    if (!cursorWindow) return 0;
    cursorWindow.opaque = NO;
    cursorWindow.backgroundColor = NSColor.clearColor;
    cursorWindow.hasShadow = NO;
    cursorWindow.ignoresMouseEvents = YES;
    cursorWindow.level = NSFloatingWindowLevel + 1;
    cursorWindow.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    cursorWindow.contentView = [[DWCursorView alloc] initWithFrame:NSMakeRect(0, 0, 18, 22)];
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
      NSPoint origin = NSMakePoint(NSMinX(screen.frame) + localX - 2,
                                   NSMaxY(screen.frame) - localY - 20);
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
