package com.osvauld.p2p

import android.app.Activity
import android.app.LocaleManager
import android.content.Context
import android.content.res.Configuration
import android.os.Build
import android.os.LocaleList
import java.util.Locale

/**
 * The app's own language (Settings › Language). Android 13+ keeps it as the per-app language, the same one the system's
 * App languages page shows. Older phones (the Pixel 3a stops at Android 12) keep it here, and every Activity, Service and
 * the Application wrap their Context with it ([wrap]). An empty tag means: follow the phone.
 */
object AppLanguage {
    private const val KEY = "tag"
    private fun prefs(c: Context) = c.getSharedPreferences("language", Context.MODE_PRIVATE)

    /** The languages this build ships, in res/xml/locales_config.xml order (written by scripts/i18n_gen.py). */
    fun shipped(c: Context): List<String> {
        val out = mutableListOf<String>()
        c.resources.getXml(R.xml.locales_config).use { p ->
            while (p.next() != org.xmlpull.v1.XmlPullParser.END_DOCUMENT) {
                if (p.eventType == org.xmlpull.v1.XmlPullParser.START_TAG && p.name == "locale") {
                    p.getAttributeValue("http://schemas.android.com/apk/res/android", "name")?.let(out::add)
                }
            }
        }
        return out
    }

    /** The chosen tag, or "" when the app follows the phone. */
    fun current(c: Context): String =
        if (Build.VERSION.SDK_INT >= 33) c.getSystemService(LocaleManager::class.java).applicationLocales.toLanguageTags().substringBefore(',')
        else prefs(c).getString(KEY, "").orEmpty()

    /** Switch to [tag] ("" = follow the phone) and redraw [activity] in it. */
    fun set(activity: Activity, tag: String) {
        if (Build.VERSION.SDK_INT >= 33) {
            activity.getSystemService(LocaleManager::class.java).applicationLocales =
                if (tag.isEmpty()) LocaleList.getEmptyLocaleList() else LocaleList.forLanguageTags(tag)
        } else {
            prefs(activity).edit().putString(KEY, tag).commit()
            // The Application's resources serve appString() and notifications; point them at the new language too.
            val app = activity.applicationContext.resources
            val config = Configuration(app.configuration).apply { setLocales(LocaleList(locale(tag) ?: systemLocale())) }
            Locale.setDefault(config.locales[0])
            @Suppress("DEPRECATION") app.updateConfiguration(config, app.displayMetrics)
        }
        // MainActivity handles locale changes itself (manifest configChanges), so redraw it explicitly.
        activity.recreate()
    }

    /** Older phones: [base] in the chosen language. Android 13+ does this itself, and without a choice nothing changes. */
    fun wrap(base: Context): Context {
        if (Build.VERSION.SDK_INT >= 33) return base
        val locale = locale(prefs(base).getString(KEY, "").orEmpty()) ?: return base
        Locale.setDefault(locale)
        return base.createConfigurationContext(Configuration(base.resources.configuration).apply { setLocales(LocaleList(locale)) })
    }

    /** [tag]'s name in its own language, as people look for it: "Deutsch", "suomi", "हिन्दी". */
    fun nativeName(tag: String): String {
        val l = Locale.forLanguageTag(tag)
        return l.getDisplayName(l).replaceFirstChar { it.titlecase(l) }
    }

    /** [tag]'s name in the language the app shows now ("German"), or null when that is the native name already. */
    fun localName(tag: String): String? {
        val here = Locale.getDefault()
        return Locale.forLanguageTag(tag).getDisplayName(here).replaceFirstChar { it.titlecase(here) }.takeIf { it != nativeName(tag) }
    }

    private fun locale(tag: String): Locale? = tag.takeIf { it.isNotEmpty() }?.let(Locale::forLanguageTag)
    private fun systemLocale(): Locale = android.content.res.Resources.getSystem().configuration.locales[0]
}
