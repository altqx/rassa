#include <ass/ass.h>
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    ASS_Library *library = ass_library_init();
    if (!library) {
        fprintf(stderr, "ass_library_init failed\n");
        return 1;
    }

    ASS_Renderer *renderer = ass_renderer_init(library);
    if (!renderer) {
        fprintf(stderr, "ass_renderer_init failed\n");
        ass_library_done(library);
        return 2;
    }

    ass_set_frame_size(renderer, 320, 240);
    ass_set_fonts(renderer, NULL, "Sans", 1, NULL, 1);

    const char script[] =
        "[Script Info]\n"
        "ScriptType: v4.00+\n"
        "PlayResX: 320\n"
        "PlayResY: 240\n"
        "[V4+ Styles]\n"
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n"
        "Style: Default,Sans,24,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1\n"
        "[Events]\n"
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
        "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,hello rassa\n";

    ASS_Track *track = ass_read_memory(library, (char *)script, (int)(sizeof(script) - 1), NULL);
    if (!track) {
        fprintf(stderr, "ass_read_memory failed\n");
        ass_renderer_done(renderer);
        ass_library_done(library);
        return 3;
    }

    int detect_change = 0;
    ASS_Image *image = ass_render_frame(renderer, track, 0, &detect_change);
    printf("version=0x%x detect_change=%d image=%s\n", ass_library_version(), detect_change, image ? "yes" : "no");

    ass_free_track(track);
    ass_renderer_done(renderer);
    ass_library_done(library);
    return 0;
}
