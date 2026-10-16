// Thin realtime wrapper over libvpx's VP8 encoder, so Rust never touches libvpx's config structs.
// One frame in, at most one packet out (same shape as aom_rt.c).
#include <stdlib.h>
#include <string.h>
#include <vpx/vp8cx.h>
#include <vpx/vpx_encoder.h>

typedef struct {
    vpx_codec_ctx_t ctx;
    vpx_codec_enc_cfg_t cfg;
    vpx_image_t img;
    unsigned w, h;
    long pts;
} tl_vpx_enc;

tl_vpx_enc *tl_vpx_enc_new(unsigned w, unsigned h, unsigned fps, unsigned kbps, unsigned threads,
                           int cpu_used, unsigned keyint) {
    vpx_codec_iface_t *iface = vpx_codec_vp8_cx();
    tl_vpx_enc *e = calloc(1, sizeof *e);
    if (!e) return NULL;
    vpx_codec_enc_cfg_t *cfg = &e->cfg;
    if (vpx_codec_enc_config_default(iface, cfg, 0) != VPX_CODEC_OK) { free(e); return NULL; }
    cfg->g_w = w;
    cfg->g_h = h;
    cfg->g_timebase.num = 1;
    cfg->g_timebase.den = fps;
    cfg->g_threads = threads;
    cfg->g_lag_in_frames = 0;          // no lookahead: a packet per frame, immediately
    cfg->g_error_resilient = 1;        // frames stay decodable across lost reference updates
    cfg->g_pass = VPX_RC_ONE_PASS;
    cfg->rc_end_usage = VPX_CBR;
    cfg->rc_target_bitrate = kbps;
    cfg->rc_dropframe_thresh = 0;      // the transport decides what to drop
    cfg->rc_buf_sz = 1000;
    cfg->rc_buf_initial_sz = 600;
    cfg->rc_buf_optimal_sz = 600;
    cfg->rc_undershoot_pct = 50;
    cfg->rc_overshoot_pct = 50;
    cfg->rc_min_quantizer = 2;
    cfg->rc_max_quantizer = 56;
    cfg->kf_mode = VPX_KF_AUTO;
    cfg->kf_min_dist = 0;
    cfg->kf_max_dist = keyint;
    e->w = w;
    e->h = h;
    if (vpx_codec_enc_init(&e->ctx, iface, cfg, 0) != VPX_CODEC_OK) { free(e); return NULL; }
    vpx_codec_control(&e->ctx, VP8E_SET_CPUUSED, cpu_used);
    vpx_codec_control(&e->ctx, VP8E_SET_NOISE_SENSITIVITY, 0);
    vpx_codec_control(&e->ctx, VP8E_SET_STATIC_THRESHOLD, 1);
    vpx_codec_control(&e->ctx, VP8E_SET_TOKEN_PARTITIONS, threads > 2 ? 2 : 0);
    return e;
}

int tl_vpx_enc_set_bitrate(tl_vpx_enc *e, unsigned kbps) {
    e->cfg.rc_target_bitrate = kbps;
    return vpx_codec_enc_config_set(&e->ctx, &e->cfg) == VPX_CODEC_OK ? 0 : -1;
}

// `i420` is tightly packed. The packet is copied to `out` (capacity `cap`); returns its size,
// 0 if the encoder produced nothing, -1 on error, -2 if `cap` is too small. `*key` is set.
long tl_vpx_enc_encode(tl_vpx_enc *e, const unsigned char *i420, int force_key, unsigned char *out,
                       size_t cap, int *key) {
    if (!vpx_img_wrap(&e->img, VPX_IMG_FMT_I420, e->w, e->h, 1, (unsigned char *)i420)) return -1;
    vpx_enc_frame_flags_t flags = force_key ? VPX_EFLAG_FORCE_KF : 0;
    if (vpx_codec_encode(&e->ctx, &e->img, e->pts++, 1, flags, VPX_DL_REALTIME) != VPX_CODEC_OK)
        return -1;
    vpx_codec_iter_t it = NULL;
    const vpx_codec_cx_pkt_t *pkt;
    size_t n = 0;
    *key = 0;
    while ((pkt = vpx_codec_get_cx_data(&e->ctx, &it))) {
        if (pkt->kind != VPX_CODEC_CX_FRAME_PKT) continue;
        if (n + pkt->data.frame.sz > cap) return -2;
        memcpy(out + n, pkt->data.frame.buf, pkt->data.frame.sz);
        n += pkt->data.frame.sz;
        if (pkt->data.frame.flags & VPX_FRAME_IS_KEY) *key = 1;
    }
    return (long)n;
}

void tl_vpx_enc_free(tl_vpx_enc *e) {
    if (!e) return;
    vpx_codec_destroy(&e->ctx);
    free(e);
}
