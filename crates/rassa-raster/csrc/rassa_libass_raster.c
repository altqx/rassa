#include "config.h"
#include "ass_compat.h"
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include <string.h>
#include <ft2build.h>
#include FT_OUTLINE_H

#include "ass_outline.h"
#include "ass_rasterizer.h"
#include "ass_bitmap_engine.h"
#include "ass_utils.h"

void ass_fill_solid_tile16_c(uint8_t *buf, ptrdiff_t stride, int set);
void ass_fill_halfplane_tile16_c(uint8_t *buf, ptrdiff_t stride, int32_t a, int32_t b, int64_t c, int32_t scale);
void ass_fill_generic_tile16_c(uint8_t *restrict buf, ptrdiff_t stride, const struct segment *restrict line, size_t n_lines, int winding);
void ass_merge_tile16_c(uint8_t *restrict buf, ptrdiff_t stride, const uint8_t *restrict tile);

typedef struct {
    int32_t width;
    int32_t height;
    int32_t stride;
    int32_t left;
    int32_t top;
    uint8_t *buffer;
} RassaLibassBitmap;

static BitmapEngine rassa_engine(void) {
    BitmapEngine engine;
    memset(&engine, 0, sizeof(engine));
    engine.align_order = 4;
    engine.tile_order = 4;
    engine.fill_solid = ass_fill_solid_tile16_c;
    engine.fill_halfplane = ass_fill_halfplane_tile16_c;
    engine.fill_generic = ass_fill_generic_tile16_c;
    engine.merge = ass_merge_tile16_c;
    return engine;
}

int rassa_libass_rasterize_outline(const FT_Outline *source, RassaLibassBitmap *out) {
    if (!source || !out) return 0;
    memset(out, 0, sizeof(*out));
    if (source->n_points <= 0 || source->n_contours <= 0) return 1;

    BitmapEngine engine = rassa_engine();
    ASS_Outline outline;
    ass_outline_clear(&outline);
    if (!ass_outline_alloc(&outline, (size_t)source->n_points * 2 + 8, (size_t)source->n_points * 2 + 8)) return 0;
    if (!ass_outline_convert(&outline, source)) {
        ass_outline_free(&outline);
        return 0;
    }

    RasterizerData rst;
    if (!ass_rasterizer_init(&engine, &rst, 16)) {
        ass_outline_free(&outline);
        return 0;
    }
    if (!ass_rasterizer_set_outline(&rst, &outline, false)) {
        ass_rasterizer_done(&rst);
        ass_outline_free(&outline);
        return 0;
    }

    int32_t x_min = (rst.bbox.x_min - 1) >> 6;
    int32_t y_min = (rst.bbox.y_min - 1) >> 6;
    int32_t x_max = (rst.bbox.x_max + 127) >> 6;
    int32_t y_max = (rst.bbox.y_max + 127) >> 6;
    int32_t w = x_max - x_min;
    int32_t h = y_max - y_min;
    if (w <= 0 || h <= 0) {
        ass_rasterizer_done(&rst);
        ass_outline_free(&outline);
        return 1;
    }

    int32_t mask = (1 << engine.tile_order) - 1;
    int32_t tile_w = (w + mask) & ~mask;
    int32_t tile_h = (h + mask) & ~mask;
    uint8_t *buffer = ass_aligned_alloc((size_t)1 << engine.align_order, (size_t)tile_w * (size_t)tile_h, true);
    if (!buffer) {
        ass_rasterizer_done(&rst);
        ass_outline_free(&outline);
        return 0;
    }

    if (!ass_rasterizer_fill(&engine, &rst, buffer, x_min, y_min, tile_w, tile_h, tile_w)) {
        ass_aligned_free(buffer);
        ass_rasterizer_done(&rst);
        ass_outline_free(&outline);
        return 0;
    }

    out->width = tile_w;
    out->height = tile_h;
    out->stride = tile_w;
    out->left = x_min;
    out->top = -y_min + 1;
    out->buffer = buffer;
    ass_rasterizer_done(&rst);
    ass_outline_free(&outline);
    return 1;
}

void rassa_libass_free_bitmap(uint8_t *buffer) {
    ass_aligned_free(buffer);
}
