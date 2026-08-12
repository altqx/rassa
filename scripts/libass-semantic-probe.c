#include <ass/ass.h>

#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static void print_image_summary(const ASS_Image *image, int index,
                                int frame_width, int frame_height)
{
    uint64_t alpha_sum = 0;
    size_t lit = 0;
    int min_x = image->w;
    int min_y = image->h;
    int max_x = -1;
    int max_y = -1;

    if (image->w < 0 || image->h < 0 || image->stride < image->w ||
        image->dst_x < 0 || image->dst_y < 0 ||
        (int64_t) image->dst_x + image->w > frame_width ||
        (int64_t) image->dst_y + image->h > frame_height ||
        (image->w > 0 && image->h > 0 && !image->bitmap)) {
        fprintf(stderr,
                "invalid ASS_Image geometry: x=%d y=%d w=%d h=%d stride=%d bitmap=%p\n",
                image->dst_x, image->dst_y, image->w, image->h,
                image->stride, (void *) image->bitmap);
        exit(65);
    }

    for (int y = 0; y < image->h; y++) {
        const uint8_t *row = image->bitmap + (size_t) y * image->stride;
        for (int x = 0; x < image->w; x++) {
            uint8_t value = row[x];
            alpha_sum += value;
            if (!value)
                continue;
            lit++;
            if (x < min_x) min_x = x;
            if (y < min_y) min_y = y;
            if (x > max_x) max_x = x;
            if (y > max_y) max_y = y;
        }
    }

    if (!lit)
        min_x = min_y = max_x = max_y = -1;
    printf("IMAGE %d %d %08" PRIx32 " %d %d %d %d %d %zu %d %d %d %d %" PRIu64 "\n",
           index, image->type, image->color, image->dst_x, image->dst_y,
           image->w, image->h, image->stride, lit,
           min_x, min_y, max_x, max_y, alpha_sum);
}

int main(int argc, char **argv)
{
    if (argc != 8) {
        fprintf(stderr,
                "usage: %s SCRIPT FONTS_DIR TIME_MS STORAGE_WIDTH "
                "STORAGE_HEIGHT FRAME_WIDTH FRAME_HEIGHT\n",
                argv[0]);
        return 64;
    }

    const char *script = argv[1];
    const char *fonts_dir = argv[2];
    long long time_ms = strtoll(argv[3], NULL, 10);
    int storage_width = (int) strtol(argv[4], NULL, 10);
    int storage_height = (int) strtol(argv[5], NULL, 10);
    int frame_width = (int) strtol(argv[6], NULL, 10);
    int frame_height = (int) strtol(argv[7], NULL, 10);
    ASS_Library *library = ass_library_init();
    if (!library)
        return 66;
    ass_set_fonts_dir(library, fonts_dir);
    ass_set_extract_fonts(library, 1);

    ASS_Track *track = ass_read_file(library, (char *) script, NULL);
    if (!track) {
        ass_library_done(library);
        return 67;
    }
    ASS_Renderer *renderer = ass_renderer_init(library);
    if (!renderer) {
        ass_free_track(track);
        ass_library_done(library);
        return 68;
    }
    ass_set_storage_size(renderer, storage_width, storage_height);
    ass_set_frame_size(renderer, frame_width, frame_height);
    ass_set_fonts(renderer, NULL, "Arial", ASS_FONTPROVIDER_AUTODETECT, NULL, 1);

    int active = 0;
    for (int i = 0; i < track->n_events; i++) {
        int64_t start = track->events[i].Start;
        int64_t end = start + track->events[i].Duration;
        if (time_ms >= start && time_ms < end)
            active++;
    }
    int detect_change = -1;
    ASS_Image *images = ass_render_frame(renderer, track, time_ms, &detect_change);
    int image_count = 0;
    printf("TRACK %d %d %d %d %d\n", track->n_styles, track->n_events,
           track->PlayResX, track->PlayResY, active);
    for (const ASS_Image *image = images; image; image = image->next)
        print_image_summary(image, image_count++, frame_width, frame_height);
    printf("END %d %d\n", image_count, detect_change);

    ass_renderer_done(renderer);
    ass_free_track(track);
    ass_library_done(library);
    return 0;
}
