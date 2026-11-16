# Dynamic Island for Android (Rust Implementation)

## 1. Technical Names
Use these exact technical names across all source files and documentation:
- **Dynamic Island**: The interactive black overlay at the top of the touch screen.
- **Android overlay window**: The system-level window that stays above all other applications.
- **Front camera cutout**: The physical camera hole in the top display area.
- **Touch screen**: The display surface that detects user finger gestures.
- **Status bar**: The top system area that shows time, network, and battery icons.
- **Live Activities**: Continuous background tasks that show real-time updates.
- **Primary pill**: The main pill-shaped black container.
- **Circular island**: The secondary circular black container in split mode.
- **Expanded view**: The large rounded rectangle that shows detailed controls.

## 2. Project Objective and Architecture
Build a persistent Android application in Rust that implements the Dynamic Island.
Use Rust as the primary programming language for all state logic, animations, and rendering.
Use the Android NDK and JNI bindings to access Android system services.
Render the user interface with hardware acceleration through Vulkan or OpenGL ES.
Run the application as a persistent Android foreground service.
Keep the Dynamic Island active permanently above the lock screen, home screen, and all applications.

## 3. Physical Shape and Hardware Integration
Read the display cutout metrics from the Android WindowInsets API at startup.
Draw the Dynamic Island as a pill-shaped black area with a pure `#000000` background.
Create two fully curved semicircular ends and two parallel horizontal edges.
Surround the front camera cutout completely inside the black area.
Blend the physical camera hole with the digital black pixels.
Remove all physical borders, drop shadows, and sharp corners.
Animate all shape and size transitions with smooth spring physics at 120 frames per second.

## 4. Position and Spatial Layout
Place the Dynamic Island at the top center of the touch screen.
Align the vertical center of the primary pill with the front camera cutout.
Keep a thin strip of active screen pixels between the top screen edge and the Dynamic Island.
Maintain a fixed top position during vertical or horizontal screen scrolling.
Detect screen rotation events and recalculate the top center coordinates immediately.

## 5. Visual States and Dimensions
Implement four distinct visual states for the Dynamic Island:

1. **Idle state**:
   - Keep the minimum pill dimensions when no Live Activities run.
   - Cover only the front camera cutout plus a small symmetrical margin.
2. **Compact state**:
   - Increase the width of the primary pill horizontally.
   - Show an application icon on the left side of the front camera cutout.
   - Show short status text, a timer, or an audio waveform on the right side.
   - Keep the center area behind the front camera cutout empty and black.
3. **Split state**:
   - Divide the Dynamic Island into two separate black shapes when two Live Activities run together.
   - Place the primary pill on the left to show the newest activity.
   - Place a detached circular island on the right to show the earlier activity.
   - Keep a clear visible gap between the primary pill and the circular island.
4. **Expanded state**:
   - Increase both the width and height of the primary pill.
   - Form a large rounded rectangle at the top of the touch screen.
   - Display album artwork, media playback buttons, progress sliders, or full notification text.

## 6. Interactive Touch Gestures
Capture touch events inside the Android overlay window bounds and pass unused touches to underlying applications.
Execute these exact actions for user finger gestures:

- **Single tap**: Open the source application in full screen when the user taps the primary pill or circular island once.
- **Touch and hold**: Open the expanded view when the user presses and holds the Dynamic Island for 400 milliseconds.
- **Horizontal swipe inward**: Hide active Live Activities when the user swipes from the outer edge toward the center. Keep the background task active and return the Dynamic Island to the idle state.
- **Horizontal swipe outward**: Restore hidden Live Activities when the user swipes from the center toward the outer edge.
- **Tap outside**: Close the expanded view and return to the compact state when the user touches any screen area below the expanded view.

## 7. Dual-Activity Split Behavior
Support two simultaneous Live Activities at the same time.
Split the primary pill into two independent touch targets when a second background task starts.
Route a single tap on the circular island to open the second application.
Route a touch and hold on the primary pill to open the expanded view for the first application.
Merge the circular island back into the primary pill when one task ends.

## 8. Android System Events and Notifications
Intercept Android system broadcasts and NotificationListenerService events in real time.
Display visual feedback inside the Dynamic Island for these system events:

- **Battery charging**: Widen the primary pill for 3 seconds to show a green charging icon and the exact battery percentage.
- **Ringer mode change**: Show a bell icon and the active mode text when the user switches between silent, vibrate, and ring modes.
- **Media playback**: Show album art on the left and animated audio bars on the right when audio plays. Provide play, pause, next, and previous buttons in the expanded view.
- **Incoming calls**: Expand the primary pill to show caller name, caller photo, accept button, and decline button.
- **Timers and navigation**: Show live countdown numbers or turn-by-turn navigation arrows in the compact state.
- **Privacy indicators**: Show an orange dot when an application accesses the microphone. Show a green dot when an application accesses the camera. Place privacy dots inside the black area next to the front camera cutout.

## 9. Permissions and Reliability
Request the `SYSTEM_ALERT_WINDOW` permission on first launch.
Request the `BIND_NOTIFICATION_LISTENER_SERVICE` permission to read active media and notifications.
Request the `FOREGROUND_SERVICE` permission to prevent the Android system from stopping the application.
Restart the foreground service automatically after a device reboot.
