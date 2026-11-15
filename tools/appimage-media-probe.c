#define _GNU_SOURCE
#include <dlfcn.h>
#include <gst/gst.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Use longstanding GLib/GStreamer APIs so the probe can run against the older
 * runtime bundled by the release builder, even when compiled on a newer host. */
static const char *appdir;
static gint decoded_buffers;

static gboolean require_bundled_path(const char *label, const char *path) {
  char *resolved = path ? realpath(path, NULL) : NULL;
  size_t root_length = strlen(appdir);
  gboolean bundled = resolved && strncmp(resolved, appdir, root_length) == 0 &&
                     resolved[root_length] == '/';
  if (!bundled) {
    fprintf(stderr, "%s is outside the AppImage: %s\n", label,
            path ? path : "(unset)");
  } else {
    printf("%s: %s\n", label, resolved + root_length + 1);
  }
  free(resolved);
  return bundled;
}

static gboolean require_bundled_symbol(const char *label, void *symbol) {
  Dl_info library;
  if (!dladdr(symbol, &library)) {
    fprintf(stderr, "Cannot locate %s\n", label);
    return FALSE;
  }
  return require_bundled_path(label, library.dli_fname);
}

static gboolean require_factory(const char *name) {
  GstElement *element = gst_element_factory_make(name, NULL);
  if (!element) {
    fprintf(stderr, "Missing or unloadable GStreamer element: %s\n", name);
    return FALSE;
  }
  GstElementFactory *factory = gst_element_get_factory(element);
  GstPlugin *plugin = gst_plugin_feature_get_plugin(GST_PLUGIN_FEATURE(factory));
  gboolean bundled = plugin &&
      require_bundled_path(name, gst_plugin_get_filename(plugin));
  if (!plugin) {
    fprintf(stderr, "Cannot locate plugin for %s\n", name);
  }
  if (plugin) {
    gst_object_unref(plugin);
  }
  gst_object_unref(element);
  return bundled;
}

static gboolean check_loaded_libraries(void) {
  FILE *maps = fopen("/proc/self/maps", "r");
  if (!maps) {
    perror("/proc/self/maps");
    return FALSE;
  }
  char *line = NULL;
  size_t capacity = 0;
  gboolean bundled = TRUE;
  while (getline(&line, &capacity, maps) != -1) {
    char *path = strchr(line, '/');
    if (!path) {
      continue;
    }
    path[strcspn(path, "\n")] = '\0';
    const char *name = strrchr(path, '/') + 1;
    if (strncmp(name, "libgst", 6) != 0 &&
        strncmp(name, "libglib-2.0", 11) != 0 &&
        strncmp(name, "libgobject-2.0", 14) != 0) {
      continue;
    }
    char *resolved = realpath(path, NULL);
    size_t root_length = strlen(appdir);
    if (!resolved || strncmp(resolved, appdir, root_length) != 0 ||
        resolved[root_length] != '/') {
      fprintf(stderr, "Loaded media dependency outside the AppImage: %s\n", path);
      bundled = FALSE;
    }
    free(resolved);
  }
  free(line);
  fclose(maps);
  return bundled;
}

static void on_handoff(GstElement *sink, GstBuffer *buffer, GstPad *pad,
                       gpointer user_data) {
  (void)sink;
  (void)user_data;
  GstCaps *caps = gst_pad_get_current_caps(pad);
  if (caps && gst_caps_get_size(caps) > 0 && gst_buffer_get_size(buffer) > 0 &&
      gst_structure_has_name(gst_caps_get_structure(caps, 0), "audio/x-raw")) {
    g_atomic_int_inc(&decoded_buffers);
  }
  if (caps) {
    gst_caps_unref(caps);
  }
}

static gboolean decode_wav(const char *filename) {
  GError *error = NULL;
  GstElement *pipeline = gst_parse_launch(
      "filesrc name=source ! decodebin ! audioconvert ! "
      "fakesink name=sink sync=false signal-handoffs=true", &error);
  if (!pipeline || error) {
    fprintf(stderr, "Cannot create decoding pipeline: %s\n",
            error ? error->message : "unknown error");
    g_clear_error(&error);
    if (pipeline) {
      gst_object_unref(pipeline);
    }
    return FALSE;
  }
  GstElement *source = gst_bin_get_by_name(GST_BIN(pipeline), "source");
  GstElement *sink = gst_bin_get_by_name(GST_BIN(pipeline), "sink");
  g_object_set(source, "location", filename, NULL);
  g_signal_connect(sink, "handoff", G_CALLBACK(on_handoff), NULL);
  gst_object_unref(source);
  gst_object_unref(sink);

  GstBus *bus = gst_element_get_bus(pipeline);
  GstStateChangeReturn state = gst_element_set_state(pipeline, GST_STATE_PLAYING);
  GstMessage *message = gst_bus_timed_pop_filtered(
      bus, 10 * GST_SECOND, GST_MESSAGE_EOS | GST_MESSAGE_ERROR);
  gboolean success = state != GST_STATE_CHANGE_FAILURE && message &&
                     GST_MESSAGE_TYPE(message) == GST_MESSAGE_EOS &&
                     g_atomic_int_get(&decoded_buffers) > 0;
  if (message && GST_MESSAGE_TYPE(message) == GST_MESSAGE_ERROR) {
    gchar *debug = NULL;
    gst_message_parse_error(message, &error, &debug);
    fprintf(stderr, "WAV decoding failed: %s\n%s\n", error->message,
            debug ? debug : "");
    g_clear_error(&error);
    g_free(debug);
  } else if (!success) {
    fprintf(stderr, "WAV decoding did not produce raw audio and EOS in time.\n");
  }
  gst_element_set_state(pipeline, GST_STATE_NULL);
  if (message) {
    gst_message_unref(message);
  }
  gst_object_unref(bus);
  gst_object_unref(pipeline);
  printf("Decoded WAV buffers: %d\n", g_atomic_int_get(&decoded_buffers));
  return success;
}

int main(int argc, char **argv) {
  if (argc != 3) {
    fprintf(stderr, "Usage: media-probe APPDIR INPUT.wav\n");
    return 2;
  }
  char *resolved_appdir = realpath(argv[1], NULL);
  if (!resolved_appdir) {
    perror("APPDIR");
    return 1;
  }
  appdir = resolved_appdir;
  gboolean success = require_bundled_symbol("GStreamer", (void *)gst_init_check);
  success = require_bundled_symbol("GLib", (void *)g_malloc) && success;
  success = require_bundled_symbol("GObject", (void *)g_object_unref) && success;
  const char *scanner = getenv("GST_PLUGIN_SCANNER_1_0");
  if (!scanner) {
    scanner = getenv("GST_PLUGIN_SCANNER");
  }
  success = require_bundled_path("GStreamer scanner", scanner) && success;
  if (!scanner || access(scanner, X_OK) != 0) {
    fprintf(stderr, "The packaged GStreamer scanner is not executable.\n");
    success = FALSE;
  }
  if (!success) {
    free(resolved_appdir);
    return 1;
  }

  GError *error = NULL;
  if (!gst_init_check(NULL, NULL, &error)) {
    fprintf(stderr, "GStreamer initialization failed: %s\n", error->message);
    g_clear_error(&error);
    free(resolved_appdir);
    return 1;
  }
  guint major, minor, micro, nano;
  gst_version(&major, &minor, &micro, &nano);
  printf("Packaged GStreamer: %u.%u.%u.%u\n", major, minor, micro, nano);
  const char *factories[] = {
      "appsink", "appsrc", "audioconvert", "audioresample", "wavparse",
      "decodebin", "vorbisdec", "mpg123audiodec", "qtdemux", "avdec_aac"};
  for (size_t i = 0; i < G_N_ELEMENTS(factories); i++) {
    success = require_factory(factories[i]) && success;
  }
  success = decode_wav(argv[2]) && success;
  success = check_loaded_libraries() && success;
  gst_deinit();
  free(resolved_appdir);
  return success ? 0 : 1;
}
