package com.primetime.app.widget

import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews
import com.primetime.app.R

class ScarcityClockWidget : AppWidgetProvider() {

    override fun onUpdate(context: Context, appWidgetManager: AppWidgetManager, appWidgetIds: IntArray) {
        for (appWidgetId in appWidgetIds) {
            updateAppWidget(context, appWidgetManager, appWidgetId)
        }
    }

    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        if (intent.action == "primetime.action.UPDATE_WIDGET") {
            val appWidgetManager = AppWidgetManager.getInstance(context)
            val thisWidget = ComponentName(context, ScarcityClockWidget::class.java)
            val appWidgetIds = appWidgetManager.getAppWidgetIds(thisWidget)
            onUpdate(context, appWidgetManager, appWidgetIds)
        }
    }
}

internal fun updateAppWidget(context: Context, appWidgetManager: AppWidgetManager, appWidgetId: Int) {
    val prefs = context.getSharedPreferences("WidgetPrefs", Context.MODE_PRIVATE)
    val days = prefs.getLong("days", 0)
    val degrees = prefs.getLong("degrees", 0)
    val twins = prefs.getLong("twins", 0)

    val views = RemoteViews(context.packageName, R.layout.widget_scarcity_clock)
    views.setTextViewText(R.id.textDays, String.format("%02d", days))
    views.setTextViewText(R.id.textDegrees, String.format("%02d", degrees))
    views.setTextViewText(R.id.textTwins, String.format("%02d", twins))

    appWidgetManager.updateAppWidget(appWidgetId, views)
}
