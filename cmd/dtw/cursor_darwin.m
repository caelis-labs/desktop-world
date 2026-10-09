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
  // A compact paper plane with its nose anchored to the delivered point.
  // It has no halo or background disk that could cover the target.
  NSBezierPath *plane = [NSBezierPath bezierPath];
  [plane moveToPoint:NSMakePoint(2, 20)];
  [plane lineToPoint:NSMakePoint(15.5, 13.8)];
  [plane curveToPoint:NSMakePoint(15.9, 11.5)
          controlPoint1:NSMakePoint(16.5, 13.3) controlPoint2:NSMakePoint(16.5, 12.2)];
  [plane lineToPoint:NSMakePoint(10.4, 11)];
  [plane lineToPoint:NSMakePoint(11.7, 4.9)];
  [plane curveToPoint:NSMakePoint(9.1, 3.4)
          controlPoint1:NSMakePoint(12, 3.5) controlPoint2:NSMakePoint(10.3, 2.8)];
  [plane lineToPoint:NSMakePoint(2.8, 11.3)];
  [plane closePath];
  NSGradient *tint = [[NSGradient alloc] initWithColorsAndLocations:
    [NSColor colorWithSRGBRed:.28 green:.83 blue:1 alpha:.97], 0.0,
    [NSColor colorWithSRGBRed:.58 green:.48 blue:.98 alpha:.97], .52,
    [NSColor colorWithSRGBRed:.98 green:.71 blue:.86 alpha:.97], 1.0, nil];
  [tint drawInBezierPath:plane angle:-42];
  [[NSColor colorWithSRGBRed:1 green:1 blue:1 alpha:.75] setStroke];
  plane.lineWidth = .65;
  plane.lineJoinStyle = NSLineJoinStyleRound;
  [plane stroke];
  NSBezierPath *fold = [NSBezierPath bezierPath];
  [fold moveToPoint:NSMakePoint(4.7, 16.8)];
  [fold lineToPoint:NSMakePoint(9.4, 11.3)];
  [fold lineToPoint:NSMakePoint(10.1, 5.8)];
  fold.lineWidth = 1.0;
  fold.lineCapStyle = NSLineCapStyleRound;
  fold.lineJoinStyle = NSLineJoinStyleRound;
  [[NSColor colorWithSRGBRed:1 green:1 blue:1 alpha:.87] setStroke];
  [fold stroke];
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
