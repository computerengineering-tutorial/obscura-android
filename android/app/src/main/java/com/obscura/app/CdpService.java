package com.obscura.app;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;
import android.util.Log;

public class CdpService extends Service {
    private static final String TAG = "CdpService";
    
    public static final String ACTION_START_CDP = "com.obscura.app.START_CDP";
    public static final String ACTION_STOP_CDP = "com.obscura.app.STOP_CDP";
    
    public static final String ACTION_START_MCP = "com.obscura.app.START_MCP";
    public static final String ACTION_STOP_MCP = "com.obscura.app.STOP_MCP";
    
    public static final String EXTRA_PORT = "extra_port";

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && intent.getAction() != null) {
            String action = intent.getAction();
            int port = intent.getIntExtra(EXTRA_PORT, 0);

            if (!ObscuraBridge.isLibraryLoaded()) {
                Log.w(TAG, "Cannot handle action " + action + ": native library not loaded");
                return START_NOT_STICKY;
            }

            switch (action) {
                case ACTION_START_CDP: {
                    int cdpPort = port > 0 ? port : 9222;
                    boolean ok = ObscuraBridge.startCdp(cdpPort);
                    Log.i(TAG, "CDP service started on port " + cdpPort + ": " + ok);
                    break;
                }
                case ACTION_STOP_CDP: {
                    ObscuraBridge.stopCdp();
                    Log.i(TAG, "CDP service stopped");
                    checkSelfStop();
                    break;
                }
                case ACTION_START_MCP: {
                    int mcpPort = port > 0 ? port : 8080;
                    boolean ok = ObscuraBridge.startMcp(mcpPort);
                    Log.i(TAG, "MCP service started on port " + mcpPort + ": " + ok);
                    break;
                }
                case ACTION_STOP_MCP: {
                    ObscuraBridge.stopMcp();
                    Log.i(TAG, "MCP service stopped");
                    checkSelfStop();
                    break;
                }
            }
        }
        return START_NOT_STICKY;
    }

    private void checkSelfStop() {
        if (!ObscuraBridge.isCdpRunning() && !ObscuraBridge.isMcpRunning()) {
            stopSelf();
        }
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @Override
    public void onDestroy() {
        if (ObscuraBridge.isLibraryLoaded()) {
            if (ObscuraBridge.isCdpRunning()) {
                ObscuraBridge.stopCdp();
            }
            if (ObscuraBridge.isMcpRunning()) {
                ObscuraBridge.stopMcp();
            }
        }
        super.onDestroy();
    }
}
