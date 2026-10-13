package com.walker.duckfoot;

import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.database.Cursor;
import android.graphics.Rect;
import android.net.Uri;
import android.os.Bundle;
import android.os.Environment;
import android.provider.DocumentsContract;
import android.provider.OpenableColumns;
import android.util.Log;
import android.view.WindowInsets;
import android.view.WindowInsetsController;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

/**
 * The Android half of the platform layer (crates/duckfoot-android/src/lib.rs is the
 * other). Everything the app draws is Rust; this class only does what needs the Android
 * framework: system file pickers, "Open with", and hiding the system bars.
 *
 * Picked documents are copied into the app's private library directory and their paths
 * handed to Rust, because the app keys photos and their sidecars on a file path and a
 * content:// document has none. Copies are reused when the same photo is picked again,
 * so its sidecar (its edits, rating and flag) comes back with it.
 *
 * Rust calls the public methods below on its event-loop thread. Answers queue in
 * {@link #events} and Rust drains them with {@link #pollEvent}; {@link #nativeWake}
 * only tells it to look.
 */
public class DuckfootActivity extends NativeActivity {
    private static final String TAG = "duckfoot";

    // Shared with crates/duckfoot-android (PICK_*).
    private static final int PICK_OPEN = 0;
    private static final int PICK_ADD = 1;
    private static final int PICK_FOLDER = 2;
    private static final int PICK_PRESET = 3;

    /** Enough of a file to tell two photos of the same name and size apart. */
    private static final int HEAD_BYTES = 64 * 1024;

    private final ArrayDeque<String[]> events = new ArrayDeque<>();
    private final ExecutorService importer = Executors.newSingleThreadExecutor();
    private final CountDownLatch nativeReady = new CountDownLatch(1);
    private volatile Set<String> extensions = Collections.emptySet();

    /** Registered by Rust (RegisterNatives) before it calls {@link #onNativeReady}. */
    private native void nativeWake();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        hideSystemBars();
        handleViewIntent(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleViewIntent(intent);
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        // The bars come back after a picker or a swipe; hide them again on return.
        if (hasFocus) {
            hideSystemBars();
        }
    }

    /** The system is short of memory: Rust lets go of what it can make again. */
    @Override
    public void onTrimMemory(int level) {
        super.onTrimMemory(level);
        if (level >= TRIM_MEMORY_RUNNING_LOW) {
            post("trim");
        }
    }

    @Override
    protected void onDestroy() {
        importer.shutdown();
        super.onDestroy();
    }

    private void hideSystemBars() {
        WindowInsetsController controller = getWindow().getInsetsController();
        if (controller == null) {
            return;
        }
        controller.hide(WindowInsets.Type.systemBars());
        controller.setSystemBarsBehavior(
                WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
    }

    // ── Called from Rust ─────────────────────────────────────────────────────────────

    /** Rust is running and {@link #nativeWake} is registered. */
    public void onNativeReady(String[] supportedExtensions) {
        extensions = new HashSet<>(Arrays.asList(supportedExtensions));
        nativeReady.countDown();
        synchronized (events) {
            if (!events.isEmpty()) {
                nativeWake();
            }
        }
    }

    /** Where imported photos and their sidecars live. Private to the app. */
    public String libraryDir() {
        return new File(getFilesDir(), "library").getAbsolutePath();
    }

    /** The shared Pictures directory; exports go in a Duckfoot folder inside it. */
    public String picturesDir() {
        File pictures = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_PICTURES);
        File duckfoot = new File(pictures, "Duckfoot");
        if (!duckfoot.exists()) {
            duckfoot.mkdirs();
        }
        return duckfoot.getAbsolutePath();
    }

    /**
     * Offer an exported photo to other apps through the system share sheet. The file is in
     * the shared Pictures folder, so the media scanner hands back a content:// URI for it
     * that any app may be granted, and no FileProvider is needed.
     */
    public void shareFile(String path) {
        try {
            android.media.MediaScannerConnection.scanFile(
                    getApplicationContext(),
                    new String[]{path},
                    null,
                    (scannedPath, uri) -> runOnUiThread(() -> {
                        if (uri == null) {
                            post("status", "Could not share: the gallery has not indexed the photo");
                            return;
                        }
                        Intent send = new Intent(Intent.ACTION_SEND);
                        send.setType(mimeFor(scannedPath));
                        send.putExtra(Intent.EXTRA_STREAM, uri);
                        send.setClipData(ClipData.newRawUri("", uri));
                        send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                        try {
                            startActivity(Intent.createChooser(send, null));
                        } catch (ActivityNotFoundException e) {
                            post("status", "No app here can take a photo");
                        }
                    }));
        } catch (Exception e) {
            Log.e(TAG, "share failed for " + path, e);
        }
    }

    private static String mimeFor(String path) {
        String lower = path.toLowerCase(Locale.ROOT);
        if (lower.endsWith(".png")) return "image/png";
        if (lower.endsWith(".webp")) return "image/webp";
        if (lower.endsWith(".tif") || lower.endsWith(".tiff")) return "image/tiff";
        if (lower.endsWith(".avif")) return "image/avif";
        if (lower.endsWith(".jxl")) return "image/jxl";
        return "image/jpeg";
    }

    /**
     * Ask the system media scanner to index an exported photo so it appears immediately
     * in the Android Gallery and Google Photos.
     */
    public void scanMediaFile(String path) {
        try {
            android.media.MediaScannerConnection.scanFile(
                    getApplicationContext(),
                    new String[]{path},
                    null,
                    (scannedPath, uri) -> Log.i(TAG, "MediaScanner indexed: " + scannedPath + " -> " + uri));
        } catch (Exception e) {
            Log.e(TAG, "MediaScanner failed for " + path, e);
        }
    }

    /** The oldest unread answer as {kind, args...}, or null. */
    public String[] pollEvent() {
        synchronized (events) {
            return events.pollFirst();
        }
    }

    public void requestPick(int kind) {
        runOnUiThread(() -> startPicker(kind));
    }

    /**
     * The Rust side has stopped for good (its edits are flushed): end the task and the
     * process, so the next launch starts cold rather than in a process whose native half
     * has already run once.
     */
    public void die() {
        runOnUiThread(() -> {
            finishAndRemoveTask();
            android.os.Process.killProcess(android.os.Process.myPid());
        });
    }

    /**
     * Where an edge swipe is the app's (the tab bar, the module strip) rather than the
     * system's Back: {@code left, top, right, bottom} in window pixels, four ints each.
     * Android honours up to 200 dp of each edge.
     */
    public void setGestureExclusion(int[] flat) {
        List<Rect> rects = new ArrayList<>();
        for (int i = 0; i + 3 < flat.length; i += 4) {
            rects.add(new Rect(flat[i], flat[i + 1], flat[i + 2], flat[i + 3]));
        }
        runOnUiThread(() -> getWindow().getDecorView().setSystemGestureExclusionRects(rects));
    }

    /** Back with nothing left to close: background the app, keeping its state. */
    public void leave() {
        runOnUiThread(() -> moveTaskToBack(true));
    }

    // ── Pickers ──────────────────────────────────────────────────────────────────────

    private void startPicker(int kind) {
        Intent intent;
        if (kind == PICK_FOLDER) {
            intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        } else {
            intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            // RAW files are labelled inconsistently (image/x-sony-arw, image/x-dcraw,
            // application/octet-stream...), so offer everything and check the extension.
            intent.setType("*/*");
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, kind == PICK_ADD);
        }
        try {
            startActivityForResult(intent, kind);
        } catch (ActivityNotFoundException e) {
            post("status", "This device has no file picker.");
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (resultCode != RESULT_OK || data == null) {
            return;
        }
        if (requestCode == PICK_FOLDER) {
            Uri tree = data.getData();
            if (tree != null) {
                importer.execute(() -> importTree(tree));
            }
            return;
        }
        if (requestCode == PICK_PRESET) {
            Uri preset = data.getData();
            if (preset != null) {
                importer.execute(() -> importPreset(preset));
            }
            return;
        }
        List<Uri> uris = new ArrayList<>();
        ClipData clip = data.getClipData();
        if (clip != null) {
            for (int i = 0; i < clip.getItemCount(); i++) {
                uris.add(clip.getItemAt(i).getUri());
            }
        } else if (data.getData() != null) {
            uris.add(data.getData());
        }
        String kind = requestCode == PICK_OPEN ? "open" : "add";
        importer.execute(() -> importDocuments(kind, uris));
    }

    @SuppressWarnings("deprecation") // getParcelableExtra(String, Class) is API 33+.
    private void handleViewIntent(Intent intent) {
        if (intent == null) {
            return;
        }
        Uri uri = null;
        if (Intent.ACTION_VIEW.equals(intent.getAction())) {
            uri = intent.getData();
        } else if (Intent.ACTION_SEND.equals(intent.getAction())) {
            uri = intent.getParcelableExtra(Intent.EXTRA_STREAM);
        }
        if (uri != null) {
            List<Uri> one = Collections.singletonList(uri);
            importer.execute(() -> importDocuments("open", one));
        }
    }

    // ── Importing ────────────────────────────────────────────────────────────────────

    private void importDocuments(String kind, List<Uri> uris) {
        if (!awaitNative()) {
            return;
        }
        File library = new File(libraryDir());
        if (!library.isDirectory() && !library.mkdirs()) {
            post("status", "Could not create the library folder.");
            return;
        }
        List<String> event = new ArrayList<>();
        event.add(kind);
        int unsupported = 0;
        for (Uri uri : uris) {
            try {
                Doc doc = describe(uri);
                if (!isSupported(doc.name)) {
                    unsupported++;
                    continue;
                }
                event.add(copyInto(library, doc, uri).getAbsolutePath());
            } catch (IOException | RuntimeException e) {
                Log.e(TAG, "import failed: " + uri, e);
                post("status", "Could not import " + uri.getLastPathSegment() + ": " + e.getMessage());
            }
        }
        if (unsupported > 0) {
            post("status", unsupported + " file(s) skipped: not a supported photo.");
        }
        if (event.size() > 1) {
            post(event.toArray(new String[0]));
        }
    }

    /**
     * A preset file, copied where Rust can read it; Rust checks it is one and adds it to
     * its presets folder.
     */
    private void importPreset(Uri uri) {
        if (!awaitNative()) {
            return;
        }
        File dir = new File(getCacheDir(), "presets");
        if (!dir.isDirectory() && !dir.mkdirs()) {
            post("status", "Could not create a folder for the preset.");
            return;
        }
        try {
            Doc doc = describe(uri);
            File copy = copyInto(dir, doc, uri);
            post("preset", copy.getAbsolutePath());
        } catch (IOException | RuntimeException e) {
            Log.e(TAG, "preset import failed: " + uri, e);
            post("status", "Could not read the preset: " + e.getMessage());
        }
    }

    /** One level of a picked folder, like the desktop's folder scan. */
    private void importTree(Uri tree) {
        if (!awaitNative()) {
            return;
        }
        try {
            String rootId = DocumentsContract.getTreeDocumentId(tree);
            String folder = describe(DocumentsContract.buildDocumentUriUsingTree(tree, rootId)).name;
            File dir = new File(libraryDir(), folder);
            if (!dir.isDirectory() && !dir.mkdirs()) {
                post("status", "Could not create " + dir);
                return;
            }
            post("status", "Importing " + folder + "…");

            Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, rootId);
            String[] columns = {
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
                DocumentsContract.Document.COLUMN_SIZE,
            };
            int copied = 0;
            try (Cursor c = getContentResolver().query(children, columns, null, null, null)) {
                while (c != null && c.moveToNext()) {
                    if (DocumentsContract.Document.MIME_TYPE_DIR.equals(c.getString(2))) {
                        continue;
                    }
                    Doc doc = new Doc(safeName(c.getString(1)), c.isNull(3) ? -1 : c.getLong(3));
                    if (!isSupported(doc.name)) {
                        continue;
                    }
                    Uri uri = DocumentsContract.buildDocumentUriUsingTree(tree, c.getString(0));
                    try {
                        copyInto(dir, doc, uri);
                        copied++;
                    } catch (IOException | RuntimeException e) {
                        Log.e(TAG, "import failed: " + uri, e);
                    }
                }
            }
            post("status", copied == 0
                    ? "No supported photos in " + folder + "."
                    : "Imported " + copied + " photo(s) from " + folder + ".");
            post("folder", dir.getAbsolutePath());
        } catch (RuntimeException e) {
            Log.e(TAG, "folder import failed: " + tree, e);
            post("status", "Could not read that folder: " + e.getMessage());
        }
    }

    private static final class Doc {
        final String name;
        final long size;

        Doc(String name, long size) {
            this.name = name;
            this.size = size;
        }
    }

    private Doc describe(Uri uri) {
        String name = null;
        long size = -1;
        String[] columns = {OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE};
        try (Cursor c = getContentResolver().query(uri, columns, null, null, null)) {
            if (c != null && c.moveToFirst()) {
                if (!c.isNull(0)) {
                    name = c.getString(0);
                }
                if (!c.isNull(1)) {
                    size = c.getLong(1);
                }
            }
        }
        return new Doc(safeName(name != null ? name : uri.getLastPathSegment()), size);
    }

    /**
     * Copy {@code uri} to {@code dir/doc.name}. A copy already there is reused when it is
     * the same photo — same size and same first 64 KB, which holds the EXIF capture time —
     * so picking a photo again brings back its sidecar. A different photo with the same
     * name gets "name (2).ext".
     */
    /** What a copy must leave free, beyond itself, so the phone is not filled to the brim. */
    private static final long SPARE_BYTES = 500L << 20;

    private File copyInto(File dir, Doc doc, Uri uri) throws IOException {
        if (doc.size >= 0 && dir.getUsableSpace() < doc.size + SPARE_BYTES) {
            throw new IOException("not enough free space to copy it in");
        }
        File dest = new File(dir, doc.name);
        if (dest.isFile()) {
            if (doc.size >= 0 && dest.length() == doc.size && sameHead(uri, dest)) {
                return dest;
            }
            dest = unusedName(dir, doc.name);
        }
        File part = new File(dir, "." + dest.getName() + ".part");
        try (InputStream in = open(uri); OutputStream out = new FileOutputStream(part)) {
            byte[] buf = new byte[1 << 16];
            int n;
            while ((n = in.read(buf)) > 0) {
                out.write(buf, 0, n);
            }
        } catch (IOException | RuntimeException e) {
            part.delete();
            throw e;
        }
        if (!part.renameTo(dest)) {
            part.delete();
            throw new IOException("could not move the copy into place");
        }
        return dest;
    }

    private InputStream open(Uri uri) throws IOException {
        InputStream in = getContentResolver().openInputStream(uri);
        if (in == null) {
            throw new IOException("the provider returned no data");
        }
        return in;
    }

    private boolean sameHead(Uri uri, File file) throws IOException {
        try (InputStream a = open(uri); InputStream b = new FileInputStream(file)) {
            return Arrays.equals(readHead(a), readHead(b));
        }
    }

    private static byte[] readHead(InputStream in) throws IOException {
        byte[] buf = new byte[HEAD_BYTES];
        int total = 0;
        int n;
        while (total < buf.length && (n = in.read(buf, total, buf.length - total)) > 0) {
            total += n;
        }
        return Arrays.copyOf(buf, total);
    }

    private static File unusedName(File dir, String name) {
        int dot = name.lastIndexOf('.');
        String stem = dot > 0 ? name.substring(0, dot) : name;
        String ext = dot > 0 ? name.substring(dot) : "";
        for (int i = 2; ; i++) {
            File candidate = new File(dir, stem + " (" + i + ")" + ext);
            if (!candidate.exists()) {
                return candidate;
            }
        }
    }

    /** A display name as a single, harmless path component. */
    private static String safeName(String name) {
        if (name == null) {
            return "photo";
        }
        name = name.replace('/', '_').replace('\\', '_').replace('\0', '_');
        if (name.isEmpty() || name.equals(".") || name.equals("..") || name.startsWith(".")) {
            return "photo" + name;
        }
        return name;
    }

    private boolean isSupported(String name) {
        int dot = name.lastIndexOf('.');
        return dot > 0 && extensions.contains(name.substring(dot + 1).toLowerCase(Locale.ROOT));
    }

    // ── To Rust ──────────────────────────────────────────────────────────────────────

    /** Imports wait for Rust, which sends the list of photo extensions when it starts. */
    private boolean awaitNative() {
        try {
            return nativeReady.await(30, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return false;
        }
    }

    private void post(String... event) {
        synchronized (events) {
            events.addLast(event);
            if (nativeReady.getCount() == 0) {
                nativeWake();
            }
        }
    }
}
