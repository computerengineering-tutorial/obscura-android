package com.obscura.app;

import android.os.Build;
import android.util.Log;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.util.Arrays;

public class ObscuraBridge {
    private static final String TAG = "ObscuraBridge";
    private static boolean sLibraryLoaded = false;
    private static String sLoadError = null;
    private static String sDiagnostics = "";

    static {
        StringBuilder diag = new StringBuilder();
        diag.append("Device: ").append(Build.MANUFACTURER).append(" ").append(Build.MODEL)
            .append(" (").append(Build.DEVICE).append(")\n")
            .append("Android: ").append(Build.VERSION.RELEASE)
            .append(" (SDK ").append(Build.VERSION.SDK_INT).append(")\n")
            .append("Supported ABIs: ").append(Arrays.toString(Build.SUPPORTED_ABIS)).append("\n");

        Log.i(TAG, "=== Initializing ObscuraBridge ===");
        Log.i(TAG, diag.toString().trim());

        try {
            Log.i(TAG, "Attempting System.loadLibrary(\"obscura_android\")...");
            long start = System.currentTimeMillis();
            System.loadLibrary("obscura_android");
            long elapsed = System.currentTimeMillis() - start;
            sLibraryLoaded = true;
            Log.i(TAG, "libobscura_android.so loaded successfully in " + elapsed + "ms");
            diag.append("Native library: Loaded successfully (").append(elapsed).append("ms)\n");
        } catch (Throwable t) {
            sLibraryLoaded = false;
            StringWriter sw = new StringWriter();
            t.printStackTrace(new PrintWriter(sw));
            sLoadError = sw.toString();
            Log.e(TAG, "FAILED to load libobscura_android.so: " + t.getMessage(), t);
            diag.append("Native library: Load FAILED\n").append("Error: ").append(t.getMessage()).append("\n");
        }
        sDiagnostics = diag.toString();
    }

    public static boolean isLibraryLoaded() {
        return sLibraryLoaded;
    }

    public static String getLoadError() {
        return sLoadError;
    }

    public static String getDiagnostics() {
        return sDiagnostics;
    }

    // ── Headless Browser APIs ───────────────────────────────────────────────

    /**
     * Fetch rendered HTML from a given URL using Obscura's engine.
     */
    public static String fetchHtml(String url) {
        return fetchHtml(url, null);
    }

    /**
     * Fetch rendered HTML from URL with custom options (JSON: proxy, stealth, user_agent, settle_ms).
     */
    public static String fetchHtml(String url, String optionsJson) {
        if (!sLibraryLoaded) {
            Log.e(TAG, "fetchHtml called but native library is not loaded");
            return "Error: Native library not loaded";
        }
        try {
            return nativeFetchHtmlWithOptions(url, optionsJson);
        } catch (Throwable t) {
            Log.e(TAG, "fetchHtml failed: " + t.getMessage(), t);
            return "Error: " + t.getMessage();
        }
    }

    /**
     * Execute JavaScript inside a rendered web page and return the result as JSON.
     */
    public static String evaluateJs(String url, String script, String optionsJson) {
        if (!sLibraryLoaded) return "{\"error\":\"Native library not loaded\"}";
        try {
            return nativeEvaluateJs(url, script, optionsJson);
        } catch (Throwable t) {
            Log.e(TAG, "evaluateJs failed: " + t.getMessage(), t);
            return "{\"error\":\"" + t.getMessage() + "\"}";
        }
    }

    /**
     * Extract clean readable plain text from a URL.
     */
    public static String extractText(String url, String optionsJson) {
        if (!sLibraryLoaded) return "Error: Native library not loaded";
        try {
            return nativeExtractText(url, optionsJson);
        } catch (Throwable t) {
            Log.e(TAG, "extractText failed: " + t.getMessage(), t);
            return "Error: " + t.getMessage();
        }
    }

    /**
     * Extract formatted Markdown from a URL (ideal for LLM context/RAG).
     */
    public static String extractMarkdown(String url, String optionsJson) {
        if (!sLibraryLoaded) return "Error: Native library not loaded";
        try {
            return nativeExtractMarkdown(url, optionsJson);
        } catch (Throwable t) {
            Log.e(TAG, "extractMarkdown failed: " + t.getMessage(), t);
            return "Error: " + t.getMessage();
        }
    }

    // ── Chrome DevTools Protocol (CDP) ──────────────────────────────────────

    public static boolean startCdp(int port) {
        return startCdp(port, "127.0.0.1", null, false, null);
    }

    public static boolean startCdp(int port, String host, String proxy, boolean stealth, String userAgent) {
        if (!sLibraryLoaded) {
            Log.e(TAG, "startCdp called but native library is not loaded");
            return false;
        }
        try {
            return nativeStartCdpWithOptions(port, host, proxy, stealth, userAgent);
        } catch (Throwable t) {
            Log.e(TAG, "startCdp failed: " + t.getMessage(), t);
            return false;
        }
    }

    public static boolean stopCdp() {
        if (!sLibraryLoaded) return false;
        try {
            return nativeStopCdp();
        } catch (Throwable t) {
            Log.e(TAG, "stopCdp failed: " + t.getMessage(), t);
            return false;
        }
    }

    public static boolean isCdpRunning() {
        if (!sLibraryLoaded) return false;
        try {
            return nativeIsCdpRunning();
        } catch (Throwable t) {
            return false;
        }
    }

    public static String getCdpEndpoint() {
        if (!sLibraryLoaded) return "";
        try {
            return nativeGetCdpEndpoint();
        } catch (Throwable t) {
            return "";
        }
    }

    // ── Model Context Protocol (MCP) ────────────────────────────────────────

    public static boolean startMcp(int port) {
        return startMcp(port, "127.0.0.1", null, false, null);
    }

    public static boolean startMcp(int port, String host, String proxy, boolean stealth, String userAgent) {
        if (!sLibraryLoaded) {
            Log.e(TAG, "startMcp called but native library is not loaded");
            return false;
        }
        try {
            return nativeStartMcpWithOptions(port, host, proxy, stealth, userAgent);
        } catch (Throwable t) {
            Log.e(TAG, "startMcp failed: " + t.getMessage(), t);
            return false;
        }
    }

    public static boolean stopMcp() {
        if (!sLibraryLoaded) return false;
        try {
            return nativeStopMcp();
        } catch (Throwable t) {
            Log.e(TAG, "stopMcp failed: " + t.getMessage(), t);
            return false;
        }
    }

    public static boolean isMcpRunning() {
        if (!sLibraryLoaded) return false;
        try {
            return nativeIsMcpRunning();
        } catch (Throwable t) {
            return false;
        }
    }

    public static String getMcpEndpoint() {
        if (!sLibraryLoaded) return "";
        try {
            return nativeGetMcpEndpoint();
        } catch (Throwable t) {
            return "";
        }
    }

    public static String executeMcpTool(String requestJson) {
        if (!sLibraryLoaded) return "{\"error\":\"Native library not loaded\"}";
        try {
            return nativeExecuteMcpTool(requestJson);
        } catch (Throwable t) {
            return "{\"error\":\"" + t.getMessage() + "\"}";
        }
    }

    // ── Diagnostics ─────────────────────────────────────────────────────────

    public static String getEngineDiagnostics() {
        if (!sLibraryLoaded) return "{}";
        try {
            return nativeGetEngineDiagnostics();
        } catch (Throwable t) {
            return "{\"error\":\"" + t.getMessage() + "\"}";
        }
    }

    // ── Native Method Declarations ──────────────────────────────────────────

    public static native String nativeFetchHtml(String url);
    public static native String nativeFetchHtmlWithOptions(String url, String optionsJson);
    public static native String nativeEvaluateJs(String url, String script, String optionsJson);
    public static native String nativeExtractText(String url, String optionsJson);
    public static native String nativeExtractMarkdown(String url, String optionsJson);

    public static native boolean nativeStartCdp(int port);
    public static native boolean nativeStartCdpWithOptions(int port, String host, String proxy, boolean stealth, String userAgent);
    public static native boolean nativeStopCdp();
    public static native boolean nativeIsCdpRunning();
    public static native String nativeGetCdpEndpoint();

    public static native boolean nativeStartMcp(int port);
    public static native boolean nativeStartMcpWithOptions(int port, String host, String proxy, boolean stealth, String userAgent);
    public static native boolean nativeStopMcp();
    public static native boolean nativeIsMcpRunning();
    public static native String nativeGetMcpEndpoint();
    public static native String nativeExecuteMcpTool(String requestJson);

    public static native String nativeGetEngineDiagnostics();
}
