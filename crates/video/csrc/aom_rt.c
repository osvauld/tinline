// Thin realtime wrapper over libaom's AV1 encoder, so Rust never touches libaom's config
// structs (their layout changes between releases). One frame in, at most one packet out.
#include <aom/aom_encoder.h>
#include <aom/aomcx.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    aom_codec_ctx_t ctx;
    aom_image_t img;
    unsigned w, h;
    long pts;
} tl_aom;

tl_aom *tl_aom_new(unsigned w, unsigned h, unsigned fps, unsigned kbps, unsigned threads,
                   int speed, unsigned keyint) {
    aom_codec_iface_t *iface = aom_codec_av1_cx();
    aom_codec_enc_cfg_t cfg;
    if (aom_codec_enc_config_default(iface, &cfg, AOM_USAGE_REALTIME) != AOM_CODEC_OK) return NULL;
    cfg.g_w = w;
    cfg.g_h = h;
    cfg.g_timebase.num = 1;
    cfg.g_timebase.den = fps;
    cfg.g_threads = threads;
    cfg.g_lag_in_frames = 0;           // no lookahead: a packet per frame, immediately
    cfg.g_error_resilient = 0;
    cfg.rc_end_usage = AOM_CBR;
    cfg.rc_target_bitrate = kbps;
    cfg.rc_dropframe_thresh = 0;       // the transport decides what to drop
    cfg.rc_buf_sz = 1000;
    cfg.rc_buf_initial_sz = 600;
    cfg.rc_buf_optimal_sz = 600;
    cfg.rc_undershoot_pct = 50;
    cfg.rc_overshoot_pct = 50;
    cfg.rc_min_quantizer = 10;
    cfg.rc_max_quantizer = 56;
    cfg.kf_mode = AOM_KF_AUTO;
    cfg.kf_min_dist = 0;
    cfg.kf_max_dist = keyint;

    tl_aom *e = calloc(1, sizeof *e);
    if (!e) return NULL;
    if (aom_codec_enc_init(&e->ctx, iface, &cfg, 0) != AOM_CODEC_OK) { free(e); return NULL; }
    aom_codec_control(&e->ctx, AOME_SET_CPUUSED, speed);
    aom_codec_control(&e->ctx, AV1E_SET_ROW_MT, 1);
    aom_codec_control(&e->ctx, AV1E_SET_TILE_COLUMNS, threads > 2 ? 1 : 0);
    aom_codec_control(&e->ctx, AV1E_SET_AQ_MODE, 3);   // cyclic refresh, the realtime default
    aom_codec_control(&e->ctx, AV1E_SET_ENABLE_TPL_MODEL, 0);
    aom_codec_control(&e->ctx, AV1E_SET_DELTAQ_MODE, 0);
    aom_codec_control(&e->ctx, AV1E_SET_ENABLE_ORDER_HINT, 0);
    aom_codec_control(&e->ctx, AV1E_SET_ENABLE_GLOBAL_MOTION, 0);
    aom_codec_control(&e->ctx, AV1E_SET_ENABLE_WARPED_MOTION, 0);
    aom_codec_control(&e->ctx, AV1E_SET_COEFF_COST_UPD_FREQ, 3);
    aom_codec_control(&e->ctx, AV1E_SET_MODE_COST_UPD_FREQ, 3);
    aom_codec_control(&e->ctx, AV1E_SET_MV_COST_UPD_FREQ, 3);
    e->w = w;
    e->h = h;
    return e;
}

int tl_aom_set_bitrate(tl_aom *e, unsigned kbps) {
    aom_codec_enc_cfg_t cfg = *e->ctx.config.enc;
    cfg.rc_target_bitrate = kbps;
    return aom_codec_enc_config_set(&e->ctx, &cfg) == AOM_CODEC_OK ? 0 : -1;
}

// `i420` is tightly packed. The packet is copied to `out` (capacity `cap`); returns its size,
// 0 if the encoder produced nothing, -1 on error, -2 if `cap` is too small. `*key` is set.
long tl_aom_encode(tl_aom *e, const unsigned char *i420, int force_key, unsigned char *out,
                   size_t cap, int *key) {
    if (!aom_img_wrap(&e->img, AOM_IMG_FMT_I420, e->w, e->h, 1, (unsigned char *)i420)) return -1;
    aom_enc_frame_flags_t flags = force_key ? AOM_EFLAG_FORCE_KF : 0;
    if (aom_codec_encode(&e->ctx, &e->img, e->pts++, 1, flags) != AOM_CODEC_OK) return -1;
    aom_codec_iter_t it = NULL;
    const aom_codec_cx_pkt_t *pkt;
    size_t n = 0;
    *key = 0;
    while ((pkt = aom_codec_get_cx_data(&e->ctx, &it))) {
        if (pkt->kind != AOM_CODEC_CX_FRAME_PKT) continue;
        if (n + pkt->data.frame.sz > cap) return -2;
        memcpy(out + n, pkt->data.frame.buf, pkt->data.frame.sz);
        n += pkt->data.frame.sz;
        if (pkt->data.frame.flags & AOM_FRAME_IS_KEY) *key = 1;
    }
    return (long)n;
}

void tl_aom_free(tl_aom *e) {
    if (!e) return;
    aom_codec_destroy(&e->ctx);
    free(e);
}
