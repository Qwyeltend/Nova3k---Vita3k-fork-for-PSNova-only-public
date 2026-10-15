package org.vita3k.emulator;

import android.content.Context;
import android.os.Build;
import android.view.Surface;
import android.view.MotionEvent;
import android.view.SurfaceHolder;
import android.view.View;

import org.libsdl.app.SDLSurface;
import org.vita3k.emulator.overlay.InputOverlay;

public class EmuSurface extends SDLSurface {

    private InputOverlay mOverlay;

    public InputOverlay getmOverlay() {
        return mOverlay;
    }

    public EmuSurface(Context context){
        super(context);
        mOverlay = new InputOverlay(context);
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        requestGameFrameRate(holder.getSurface());
        setSurfaceStatus(true);
        super.surfaceCreated(holder);
    }

    // Nova3K: the game runs at 60 FPS (30 without the FPS hack); ask Android for a refresh rate that is a
    // multiple of it, so every game frame stays on screen for the same time (no judder on 90/120 Hz screens).
    private static void requestGameFrameRate(Surface surface) {
        if (surface == null || !surface.isValid())
            return;
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S)
                surface.setFrameRate(60.0f, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE, Surface.CHANGE_FRAME_RATE_ALWAYS);
            else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R)
                surface.setFrameRate(60.0f, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE);
        } catch (Exception e) {
            // not supported on this device, keep the default refresh rate
        }
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        setSurfaceStatus(false);
        super.surfaceDestroyed(holder);
    }

    @Override
    public boolean onTouch(View v, MotionEvent event) {
        if(mOverlay.onTouch(v, event)){
            // priority is given to the overlay
            return true;
        }

        return super.onTouch(v, event);
    }

    public native void setSurfaceStatus(boolean surface_present);
}
