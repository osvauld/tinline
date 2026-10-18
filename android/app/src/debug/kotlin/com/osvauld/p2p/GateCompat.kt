package com.osvauld.p2p

import androidx.compose.runtime.*

/** Standalone gate screens retained for existing debug gallery callers. */
@Composable
fun NameScreen(name: String, onChange: (String) -> Unit, onBack: () -> Unit, progress: Int, onNext: () -> Unit) = Page {
    GateHead(onBack, progress to 6)
    NameStep(name, onChange, onNext)
}

@Composable
fun PassphraseStep(
    pass: String, onPass: (String) -> Unit, busy: Boolean, error: String?, progress: Int?, title: String, cta: String,
    onBack: (() -> Unit)?, onNext: () -> Unit, onSkip: () -> Unit, canSkip: Boolean = true,
    lead: String = "Optional. Anyone with your phone could otherwise open your account.",
) = Page {
    GateHead(onBack, progress?.let { it to 6 })
    GateStep(title, lead, foot = {
        GBtn(if (busy) "Encrypting…" else cta, onNext, enabled = pass.isNotEmpty(), busy = busy)
        if (canSkip) GBtn("Skip for now", onSkip, kind = GKind.Text, enabled = !busy)
    }) {
        NewPassphraseField(pass, onPass, !busy, { if (!busy && pass.isNotEmpty()) onNext() })
        ReservedLine(error.orEmpty(), color = Tin.c.er, lines = 3)
    }
}

@Composable
fun PhraseScreen(phrase: String, onNext: () -> Unit) {
    Page {
        GateHead(null, 3 to 6)
        PhraseStep(phrase, onNext)
    }
}
