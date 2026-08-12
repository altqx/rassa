#include <ass/ass.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int message_count;
static char last_message[1024];

static void message_callback(int level, const char *format, va_list arguments,
                             void *data) {
    (void)level;
    int *count = data;
    (*count)++;
    vsnprintf(last_message, sizeof(last_message), format, arguments);
    last_message[sizeof(last_message) - 1] = '\0';
}

int main(void) {
    ASS_Library *library = ass_library_init();
    if (!library) {
        fprintf(stderr, "ass_library_init failed\n");
        return 1;
    }
    ass_set_message_cb(library, message_callback, &message_count);

    if (ass_read_file(library, "target/rassa-capi-missing.ass", NULL) ||
        message_count == 0 || !strstr(last_message, "read failed")) {
        fprintf(stderr, "formatted message callback was not invoked\n");
        ass_library_done(library);
        return 2;
    }

    ASS_DefaultFontProvider *providers = NULL;
    size_t provider_count = 0;
    ass_get_available_font_providers(library, &providers, &provider_count);
    if (!providers || provider_count < 2 ||
        providers[0] != ASS_FONTPROVIDER_NONE ||
        providers[1] != ASS_FONTPROVIDER_AUTODETECT) {
        fprintf(stderr, "invalid font provider enumeration\n");
        free(providers);
        ass_library_done(library);
        return 3;
    }
    free(providers);

    char *owned = ass_malloc(16);
    if (!owned) {
        fprintf(stderr, "ass_malloc failed\n");
        ass_library_done(library);
        return 4;
    }
    owned[0] = 'o';
    owned[1] = 'k';
    owned[2] = '\0';
    ass_free(owned);
    ass_free(NULL);

    ASS_Renderer *renderer = ass_renderer_init(library);
    if (!renderer) {
        fprintf(stderr, "ass_renderer_init failed\n");
        ass_library_done(library);
        return 5;
    }

    ass_set_fonts_dir(library, "crates/rassa-test/fixtures/libass/compare/test");
    ass_set_frame_size(renderer, 320, 240);
    ass_set_fonts(renderer, NULL, "Pixel Operator Mono",
                  ASS_FONTPROVIDER_NONE, NULL, 1);
    ass_set_cache_limits(renderer, 2, 1);

    const char script[] =
        "[Script Info]\n"
        "ScriptType: v4.00+\n"
        "PlayResX: 320\n"
        "PlayResY: 240\n"
        "[V4+ Styles]\n"
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n"
        "Style: Default,Pixel Operator Mono,24,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1\n"
        "[Events]\n"
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
        "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,hello rassa\n"
        "Dialogue: 0,0:00:02.00,0:00:04.00,Default,,0,0,0,,cache remains valid\n";

    ASS_Track *track = ass_read_memory(library, (char *)script, (int)(sizeof(script) - 1), NULL);
    if (!track) {
        fprintf(stderr, "ass_read_memory failed\n");
        ass_renderer_done(renderer);
        ass_library_done(library);
        return 6;
    }

    static const char replacement_name[] = "Pixel Operator Mono";
    char *replacement_font = ass_malloc(sizeof(replacement_name));
    if (!replacement_font || track->n_styles < 1) {
        fprintf(stderr, "public track string replacement failed\n");
        ass_free(replacement_font);
        ass_free_track(track);
        ass_renderer_done(renderer);
        ass_library_done(library);
        return 7;
    }
    memcpy(replacement_font, replacement_name, sizeof(replacement_name));
    ass_free(track->styles[0].FontName);
    track->styles[0].FontName = replacement_font;

    int detect_change = 0;
    ASS_Image *image = ass_render_frame(renderer, track, 0, &detect_change);
    if (!image) {
        fprintf(stderr, "font directory did not resolve Pixel Operator Mono\n");
        ass_free_track(track);
        ass_renderer_done(renderer);
        ass_library_done(library);
        return 8;
    }
    ass_set_cache_limits(renderer, 0, 0);
    image = ass_render_frame(renderer, track, 2500, &detect_change);
    if (!image) {
        fprintf(stderr, "cache eviction/rerasterization failed\n");
        ass_free_track(track);
        ass_renderer_done(renderer);
        ass_library_done(library);
        return 9;
    }
    printf("version=0x%x detect_change=%d image=%s\n", ass_library_version(), detect_change, image ? "yes" : "no");

    ass_free_track(track);
    ass_renderer_done(renderer);
    ass_library_done(library);
    return 0;
}
