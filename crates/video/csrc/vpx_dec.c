// Thin wrapper over libvpx's VP8 decoder, so Rust never touches libvpx's structs (their layout
// is the library's business). One packet in, then the frames it completed, one by one.
#include <stdlib.h>
#include <string.h>
#include <vpx/vp8dx.h>
#include <vpx/vpx_decoder.h>

typedef struct {
    vpx_codec_ctx_t ctx;
    vpx_codec_iter_t iter;
    vpx_image_t *img;
} tl_vpx;

tl_vpx *tl_vpx_new(unsigned threads) {
    tl_vpx *d = calloc(1, sizeof *d);
    if (!d) return NULL;
    vpx_codec_dec_cfg_t cfg = {0};
    cfg.threads = threads;
    if (vpx_codec_dec_init(&d->ctx, vpx_codec_vp8_dx(), &cfg, 0) != VPX_CODEC_OK) {
        free(d);
        return NULL;
    }
    return d;
}

// Decodes one packet. 0 = ok, -1 = the decoder refused it, -2 = decoded but corrupt (references
// missing after loss: only a keyframe helps).
int tl_vpx_decode(tl_vpx *d, const unsigned char *data, size_t len) {
    d->iter = NULL;
    d->img = NULL;
    if (len == 0 || len > 0xffffffffu) return -1;
    if (vpx_codec_decode(&d->ctx, data, (unsigned)len, NULL, 0) != VPX_CODEC_OK) return -1;
    int corrupt = 0;
    if (vpx_codec_control(&d->ctx, VP8D_GET_FRAME_CORRUPTED, &corrupt) == VPX_CODEC_OK && corrupt)
        return -2;
    return 0;
}

// Next frame of the last packet: 1 with its size, 0 = none left, -1 = not 8-bit I420.
int tl_vpx_next(tl_vpx *d, unsigned *w, unsigned *h) {
    d->img = vpx_codec_get_frame(&d->ctx, &d->iter);
    if (!d->img) return 0;
    if (d->img->fmt != VPX_IMG_FMT_I420 || d->img->bit_depth != 8) return -1;
    *w = d->img->d_w;
    *h = d->img->d_h;
    return 1;
}

// Copies the frame from `tl_vpx_next` tightly packed (Y, U, V). 0 = ok, -1 = `cap` too small.
int tl_vpx_copy(tl_vpx *d, unsigned char *out, size_t cap) {
    const vpx_image_t *img = d->img;
    if (!img) return -1;
    size_t w = img->d_w, h = img->d_h, cw = (w + 1) / 2, ch = (h + 1) / 2;
    if (cap < w * h + 2 * cw * ch) return -1;
    const size_t pw[3] = {w, cw, cw}, ph[3] = {h, ch, ch};
    for (int p = 0; p < 3; p++) {
        const unsigned char *src = img->planes[p];
        for (size_t r = 0; r < ph[p]; r++) {
            memcpy(out, src + r * (size_t)img->stride[p], pw[p]);
            out += pw[p];
        }
    }
    return 0;
}

void tl_vpx_free(tl_vpx *d) {
    if (!d) return;
    vpx_codec_destroy(&d->ctx);
    free(d);
}
