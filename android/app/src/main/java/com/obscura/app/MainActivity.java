package com.obscura.app;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.graphics.Color;
import android.os.Bundle;
import android.text.TextUtils;
import android.text.method.ScrollingMovementMethod;
import android.view.View;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.ProgressBar;
import android.widget.RadioButton;
import android.widget.RadioGroup;
import android.widget.TextView;
import android.widget.Toast;

import android.app.Activity;

import org.json.JSONObject;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public class MainActivity extends Activity {

    private static final String TAG = "ObscuraApp";

    // Header Badge
    private TextView tvEngineBadge;

    // CDP Controls
    private EditText editCdpPort;
    private Button btnToggleCdp;
    private Button btnCopyCdpEndpoint;
    private TextView tvCdpStatus;
    private boolean isCdpRunning = false;

    // MCP Controls
    private EditText editMcpPort;
    private Button btnToggleMcp;
    private Button btnCopyMcpEndpoint;
    private TextView tvMcpStatus;
    private boolean isMcpRunning = false;

    // Browser Execution Controls
    private EditText editUrl;
    private RadioGroup rgMode;
    private RadioButton rbHtml, rbMarkdown, rbText, rbJs;
    private EditText editScript;
    private CheckBox cbStealth;
    private EditText editSettleMs;
    private Button btnExecute;
    private ProgressBar progressBar;

    // Output Controls
    private TextView tvOutputStatus;
    private TextView tvResult;
    private Button btnCopyOutput;
    private Button btnClearOutput;

    private final ExecutorService executor = Executors.newSingleThreadExecutor();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);
        android.util.Log.i(TAG, "MainActivity onCreate");

        initViews();
        checkEngineStatus();
        setupCdpSection();
        setupMcpSection();
        setupBrowserSection();
        setupOutputSection();
    }

    private void initViews() {
        tvEngineBadge = findViewById(R.id.tvEngineBadge);

        editCdpPort = findViewById(R.id.editCdpPort);
        btnToggleCdp = findViewById(R.id.btnToggleCdp);
        btnCopyCdpEndpoint = findViewById(R.id.btnCopyCdpEndpoint);
        tvCdpStatus = findViewById(R.id.tvCdpStatus);

        editMcpPort = findViewById(R.id.editMcpPort);
        btnToggleMcp = findViewById(R.id.btnToggleMcp);
        btnCopyMcpEndpoint = findViewById(R.id.btnCopyMcpEndpoint);
        tvMcpStatus = findViewById(R.id.tvMcpStatus);

        editUrl = findViewById(R.id.editUrl);
        rgMode = findViewById(R.id.rgMode);
        rbHtml = findViewById(R.id.rbHtml);
        rbMarkdown = findViewById(R.id.rbMarkdown);
        rbText = findViewById(R.id.rbText);
        rbJs = findViewById(R.id.rbJs);
        editScript = findViewById(R.id.editScript);
        cbStealth = findViewById(R.id.cbStealth);
        editSettleMs = findViewById(R.id.editSettleMs);
        btnExecute = findViewById(R.id.btnExecute);
        progressBar = findViewById(R.id.progressBar);

        tvOutputStatus = findViewById(R.id.tvOutputStatus);
        tvResult = findViewById(R.id.tvResult);
        tvResult.setMovementMethod(new ScrollingMovementMethod());
        btnCopyOutput = findViewById(R.id.btnCopyOutput);
        btnClearOutput = findViewById(R.id.btnClearOutput);
    }

    private void checkEngineStatus() {
        if (ObscuraBridge.isLibraryLoaded()) {
            tvEngineBadge.setText("● Rust V8 Active");
            tvEngineBadge.setTextColor(Color.parseColor("#16A34A"));
            android.util.Log.i(TAG, "Native Obscura engine loaded successfully");
        } else {
            tvEngineBadge.setText("● Not Loaded");
            tvEngineBadge.setTextColor(Color.parseColor("#DC2626"));
            String err = ObscuraBridge.getLoadError();
            tvResult.setText("Obscura Native Library Failed to Load.\n\nDiagnostics:\n" 
                + ObscuraBridge.getDiagnostics() + "\n\nError:\n" + (err != null ? err : "Unknown"));
            btnToggleCdp.setEnabled(false);
            btnToggleMcp.setEnabled(false);
            btnExecute.setEnabled(false);
        }
    }

    private void setupCdpSection() {
        btnToggleCdp.setOnClickListener(v -> {
            if (!ObscuraBridge.isLibraryLoaded()) return;

            if (!isCdpRunning) {
                int port = parsePort(editCdpPort, 9222);
                btnToggleCdp.setEnabled(false);
                executor.execute(() -> {
                    boolean ok = ObscuraBridge.startCdp(port);
                    runOnUiThread(() -> {
                        btnToggleCdp.setEnabled(true);
                        if (ok) {
                            isCdpRunning = true;
                            btnToggleCdp.setText(R.string.stop_cdp);
                            tvCdpStatus.setText(getString(R.string.cdp_status_running, port));
                            tvCdpStatus.setTextColor(Color.parseColor("#16A34A"));
                            btnCopyCdpEndpoint.setEnabled(true);
                            Toast.makeText(this, "CDP listening on port " + port, Toast.LENGTH_SHORT).show();
                        } else {
                            Toast.makeText(this, "Failed to start CDP server", Toast.LENGTH_SHORT).show();
                        }
                    });
                });
            } else {
                btnToggleCdp.setEnabled(false);
                executor.execute(() -> {
                    ObscuraBridge.stopCdp();
                    runOnUiThread(() -> {
                        btnToggleCdp.setEnabled(true);
                        isCdpRunning = false;
                        btnToggleCdp.setText(R.string.start_cdp);
                        tvCdpStatus.setText(R.string.cdp_status_stopped);
                        tvCdpStatus.setTextColor(Color.parseColor("#94A3B8"));
                        btnCopyCdpEndpoint.setEnabled(false);
                        Toast.makeText(this, "CDP Server stopped", Toast.LENGTH_SHORT).show();
                    });
                });
            }
        });

        btnCopyCdpEndpoint.setOnClickListener(v -> {
            String endpoint = ObscuraBridge.getCdpEndpoint();
            if (TextUtils.isEmpty(endpoint)) {
                int port = parsePort(editCdpPort, 9222);
                endpoint = "ws://127.0.0.1:" + port + "/devtools/browser";
            }
            copyToClipboard("Obscura CDP WebSocket", endpoint);
        });
    }

    private void setupMcpSection() {
        btnToggleMcp.setOnClickListener(v -> {
            if (!ObscuraBridge.isLibraryLoaded()) return;

            if (!isMcpRunning) {
                int port = parsePort(editMcpPort, 8080);
                btnToggleMcp.setEnabled(false);
                executor.execute(() -> {
                    boolean ok = ObscuraBridge.startMcp(port);
                    runOnUiThread(() -> {
                        btnToggleMcp.setEnabled(true);
                        if (ok) {
                            isMcpRunning = true;
                            btnToggleMcp.setText(R.string.stop_mcp);
                            tvMcpStatus.setText(getString(R.string.mcp_status_running, port));
                            tvMcpStatus.setTextColor(Color.parseColor("#0284C7"));
                            btnCopyMcpEndpoint.setEnabled(true);
                            Toast.makeText(this, "MCP listening on port " + port, Toast.LENGTH_SHORT).show();
                        } else {
                            Toast.makeText(this, "Failed to start MCP server", Toast.LENGTH_SHORT).show();
                        }
                    });
                });
            } else {
                btnToggleMcp.setEnabled(false);
                executor.execute(() -> {
                    ObscuraBridge.stopMcp();
                    runOnUiThread(() -> {
                        btnToggleMcp.setEnabled(true);
                        isMcpRunning = false;
                        btnToggleMcp.setText(R.string.start_mcp);
                        tvMcpStatus.setText(R.string.mcp_status_stopped);
                        tvMcpStatus.setTextColor(Color.parseColor("#94A3B8"));
                        btnCopyMcpEndpoint.setEnabled(false);
                        Toast.makeText(this, "MCP Server stopped", Toast.LENGTH_SHORT).show();
                    });
                });
            }
        });

        btnCopyMcpEndpoint.setOnClickListener(v -> {
            String endpoint = ObscuraBridge.getMcpEndpoint();
            if (TextUtils.isEmpty(endpoint)) {
                int port = parsePort(editMcpPort, 8080);
                endpoint = "http://127.0.0.1:" + port + "/sse";
            }
            copyToClipboard("Obscura MCP Endpoint", endpoint);
        });
    }

    private void setupBrowserSection() {
        rgMode.setOnCheckedChangeListener((group, checkedId) -> {
            if (checkedId == R.id.rbJs) {
                editScript.setVisibility(View.VISIBLE);
            } else {
                editScript.setVisibility(View.GONE);
            }
        });

        btnExecute.setOnClickListener(v -> {
            String url = editUrl.getText().toString().trim();
            if (TextUtils.isEmpty(url)) {
                Toast.makeText(this, "Please enter a URL", Toast.LENGTH_SHORT).show();
                return;
            }

            if (!ObscuraBridge.isLibraryLoaded()) {
                Toast.makeText(this, "Native library not loaded", Toast.LENGTH_SHORT).show();
                return;
            }

            int checkedId = rgMode.getCheckedRadioButtonId();
            boolean isStealth = cbStealth.isChecked();
            int settleMs = 300;
            try {
                String settleStr = editSettleMs.getText().toString().trim();
                if (!TextUtils.isEmpty(settleStr)) settleMs = Integer.parseInt(settleStr);
            } catch (Exception ignored) {}

            JSONObject options = new JSONObject();
            try {
                options.put("stealth", isStealth);
                options.put("settle_ms", settleMs);
            } catch (Exception ignored) {}

            String optionsJson = options.toString();
            String script = editScript.getText().toString().trim();

            btnExecute.setEnabled(false);
            progressBar.setVisibility(View.VISIBLE);
            tvOutputStatus.setText("Running Obscura engine...");
            tvResult.setText("Executing...");

            final int finalSettle = settleMs;
            executor.execute(() -> {
                long start = System.currentTimeMillis();
                String result;
                String modeLabel;

                try {
                    if (checkedId == R.id.rbMarkdown) {
                        modeLabel = "Markdown";
                        result = ObscuraBridge.extractMarkdown(url, optionsJson);
                    } else if (checkedId == R.id.rbText) {
                        modeLabel = "Plain Text";
                        result = ObscuraBridge.extractText(url, optionsJson);
                    } else if (checkedId == R.id.rbJs) {
                        modeLabel = "JS Eval";
                        result = ObscuraBridge.evaluateJs(url, script, optionsJson);
                    } else {
                        modeLabel = "Rendered HTML";
                        result = ObscuraBridge.fetchHtml(url, optionsJson);
                    }
                } catch (Throwable t) {
                    modeLabel = "Error";
                    result = "Execution failed: " + t.getMessage();
                }

                long elapsed = System.currentTimeMillis() - start;
                final String finalResult = result != null ? result : "null";
                final String finalMode = modeLabel;
                final int len = finalResult.length();

                runOnUiThread(() -> {
                    btnExecute.setEnabled(true);
                    progressBar.setVisibility(View.GONE);
                    tvOutputStatus.setText(finalMode + " • " + len + " chars in " + elapsed + "ms");
                    tvResult.setText(finalResult);
                });
            });
        });
    }

    private void setupOutputSection() {
        btnCopyOutput.setOnClickListener(v -> {
            String text = tvResult.getText().toString();
            if (!TextUtils.isEmpty(text)) {
                copyToClipboard("Obscura Output", text);
            }
        });

        btnClearOutput.setOnClickListener(v -> {
            tvResult.setText(R.string.output_placeholder);
            tvOutputStatus.setText(R.string.output_header);
        });
    }

    private int parsePort(EditText editText, int defaultPort) {
        String str = editText.getText().toString().trim();
        if (!TextUtils.isEmpty(str)) {
            try {
                int p = Integer.parseInt(str);
                if (p > 0 && p <= 65535) return p;
            } catch (Exception ignored) {}
        }
        return defaultPort;
    }

    private void copyToClipboard(String label, String text) {
        ClipboardManager clipboard = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        if (clipboard != null) {
            ClipData clip = ClipData.newPlainText(label, text);
            clipboard.setPrimaryClip(clip);
            Toast.makeText(this, "Copied: " + text, Toast.LENGTH_SHORT).show();
        }
    }

    @Override
    protected void onDestroy() {
        executor.shutdown();
        super.onDestroy();
    }
}
