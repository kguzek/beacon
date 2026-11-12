package uk.guzek.beacon

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.provider.Settings as SystemSettings
import android.text.format.DateFormat
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.StringRes
import androidx.compose.foundation.Image
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.util.Date
import uk.guzek.beacon.core.PublisherStatus

class SettingsActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent { BeaconTheme { SettingsScreen() } }
    }
}

@Composable
private fun BeaconTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val context = LocalContext.current
    val colors =
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
                if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
            dark ->
                darkColorScheme(
                    primary = Color(Tone.HEALTHY.argb),
                    background = Color(0xFF0C1018),
                    surface = Color(0xFF0C1018),
                )
            else -> lightColorScheme()
        }
    MaterialTheme(colorScheme = colors, content = content)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsScreen(model: SettingsViewModel = viewModel()) {
    val settings by model.settings.collectAsStateWithLifecycle()
    val status by model.status.collectAsStateWithLifecycle()
    val access by model.access.collectAsStateWithLifecycle()
    val refreshing by model.refreshing.collectAsStateWithLifecycle()

    LifecycleResumeEffect(Unit) {
        model.checkAccess()
        onPauseOrDispose {}
    }

    val context = LocalContext.current
    // From the shortcut, a denial (or "don't ask again") falls through to the
    // system settings; right after the first save it doesn't.
    var fromShortcut by remember { mutableStateOf(false) }
    val notificationPermission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (!granted && fromShortcut) context.openNotificationSettings()
            model.checkAccess()
        }
    val requestNotifications = { shortcut: Boolean ->
        fromShortcut = shortcut
        if (model.needsNotificationPermission) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else if (shortcut) {
            context.openNotificationSettings()
        }
    }

    Scaffold(topBar = { TopAppBar(title = { Text(stringResource(R.string.app_name)) }) }) { padding
        ->
        Column(
            modifier =
                Modifier.padding(padding)
                    .imePadding()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            val firstLaunch = settings.pageUrl == null
            if (firstLaunch) {
                Text(stringResource(R.string.first_launch_intro))
            } else {
                StatusCard(
                    status = status,
                    refreshing = refreshing,
                    onRefresh = model::refresh,
                    pageUrl = settings.pageUrl,
                )
                AccessShortcuts(access, onRequestNotifications = { requestNotifications(true) })
            }

            UrlSection(
                model = model,
                settings = settings,
                onSaved = { if (firstLaunch) requestNotifications(false) },
            )

            if (!firstLaunch) {
                HorizontalDivider()
                IntervalSection(settings.intervalSeconds, model::setInterval)
                ChipModeSection(settings.chipMode, model::setChipMode)
                SwitchRow(
                    title = R.string.hide_when_online,
                    summary = R.string.hide_when_online_summary,
                    checked = settings.hideWhenOnline,
                    onCheckedChange = model::setHideWhenOnline,
                )
            }
            Spacer(Modifier.size(8.dp))
        }
    }
}

@Composable
private fun StatusCard(
    status: StatusState,
    refreshing: Boolean,
    onRefresh: () -> Unit,
    pageUrl: String?,
) {
    val context = LocalContext.current
    val presentation = StatusPresentation.of(status)
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Image(
                    painter = painterResource(presentation.icon.large),
                    contentDescription = null,
                    modifier = Modifier.size(40.dp),
                )
                Spacer(Modifier.width(12.dp))
                Column {
                    Text(presentation.title, style = MaterialTheme.typography.titleMedium)
                    if (presentation.text.isNotEmpty()) {
                        Text(
                            presentation.text,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            if (status is StatusState.Loaded) {
                for (publisher in status.snapshot.publishers) PublisherRow(publisher)
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                status.checkedAtMillis?.let {
                    Text(
                        stringResource(
                            R.string.checked_at,
                            DateFormat.getTimeFormat(context).format(Date(it)),
                        ),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.weight(1f))
                if (refreshing) {
                    CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                } else {
                    TextButton(onClick = onRefresh) {
                        Text(stringResource(R.string.action_refresh))
                    }
                }
                if (pageUrl != null) {
                    TextButton(
                        onClick = {
                            context.launch(Intent(Intent.ACTION_VIEW, pageUrl.toUri()))
                        }
                    ) {
                        Text(stringResource(R.string.action_open_page))
                    }
                }
            }
        }
    }
}

@Composable
private fun PublisherRow(publisher: PublisherStatus) {
    Column {
        Row {
            Text(
                publisher.name,
                fontWeight = FontWeight.Medium,
                color = Color(PublisherPresentation.tone(publisher).argb),
            )
            Spacer(Modifier.weight(1f))
            Text(
                PublisherPresentation.summary(publisher),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        for (problem in publisher.problems.take(StatusPresentation.MAX_PROBLEMS)) {
            Text(
                PublisherPresentation.problem(problem),
                style = MaterialTheme.typography.bodySmall,
                color = Color(PublisherPresentation.tone(problem).argb),
                modifier = Modifier.padding(start = 12.dp),
            )
        }
        val hidden = publisher.problems.size - StatusPresentation.MAX_PROBLEMS
        if (hidden > 0) {
            Text(
                pluralStringResource(R.plurals.and_more, hidden, hidden),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 12.dp),
            )
        }
    }
}

@Composable
private fun AccessShortcuts(access: SystemAccess, onRequestNotifications: () -> Unit) {
    val context = LocalContext.current
    val packageUri = "package:${context.packageName}".toUri()
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (!access.notifications) {
            Shortcut(R.string.access_notifications, onRequestNotifications)
        }
        if (!access.liveUpdates && Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA) {
            Shortcut(R.string.access_live_updates) {
                context.launch(
                    Intent(SystemSettings.ACTION_APP_NOTIFICATION_PROMOTION_SETTINGS)
                        .putExtra(SystemSettings.EXTRA_APP_PACKAGE, context.packageName)
                )
            }
        }
        if (!access.exactAlarms && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            Shortcut(R.string.access_exact_alarms) {
                context.launch(
                    Intent(SystemSettings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, packageUri)
                )
            }
        }
        if (!access.unrestrictedBattery) {
            Shortcut(R.string.access_battery) {
                context.launch(
                    Intent(SystemSettings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, packageUri)
                )
            }
        }
    }
}

@Composable
private fun Shortcut(@StringRes text: Int, onClick: () -> Unit) {
    OutlinedButton(onClick = onClick, modifier = Modifier.fillMaxWidth()) {
        Text(stringResource(text))
    }
}

@Composable
private fun UrlSection(model: SettingsViewModel, settings: Settings, onSaved: () -> Unit) {
    val url by model.url.collectAsStateWithLifecycle()
    val test by model.test.collectAsStateWithLifecycle()
    val canSave = model.canSave(url, test, settings)
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedTextField(
            value = url,
            onValueChange = model::onUrlChange,
            label = { Text(stringResource(R.string.page_url)) },
            placeholder = { Text(model.placeholderUrl) },
            singleLine = true,
            keyboardOptions =
                KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            keyboardActions = KeyboardActions(onGo = { model.runTest() }),
            modifier = Modifier.fillMaxWidth(),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(
                onClick = model::runTest,
                enabled = url.isNotBlank() && test !is TestState.Running,
            ) {
                Text(stringResource(R.string.action_test))
            }
            Button(
                onClick = {
                    model.save()
                    onSaved()
                },
                enabled = canSave,
            ) {
                Text(stringResource(R.string.action_save))
            }
        }
        when (val result = test) {
            TestState.Idle -> Unit
            is TestState.Running ->
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            is TestState.Passed -> {
                val presentation = StatusPresentation.of(result.snapshot)
                Text(
                    stringResource(R.string.test_passed, result.snapshot.title, presentation.title),
                    color = Color(presentation.icon.tone.argb),
                )
            }
            is TestState.Failed ->
                Text(
                    stringResource(R.string.test_failed, result.message),
                    color = MaterialTheme.colorScheme.error,
                )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun IntervalSection(selected: Int, onSelect: (Int) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionTitle(R.string.interval, R.string.interval_summary)
        val labels = Settings.INTERVAL_CHOICES.map(::intervalLabel)
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            Settings.INTERVAL_CHOICES.forEachIndexed { index, seconds ->
                SegmentedButton(
                    selected = seconds == selected,
                    onClick = { onSelect(seconds) },
                    shape =
                        SegmentedButtonDefaults.itemShape(index, Settings.INTERVAL_CHOICES.size),
                ) {
                    Text(stringResource(labels[index].first, labels[index].second))
                }
            }
        }
    }
}

private fun intervalLabel(seconds: Int): Pair<Int, Int> =
    if (seconds < 60) R.string.interval_seconds to seconds
    else R.string.interval_minutes to seconds / 60

@Composable
private fun ChipModeSection(selected: ChipMode, onSelect: (ChipMode) -> Unit) {
    Column(Modifier.selectableGroup()) {
        SectionTitle(R.string.chip_mode, R.string.chip_mode_summary)
        for (mode in ChipMode.entries) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier =
                    Modifier.fillMaxWidth()
                        .selectable(
                            selected = mode == selected,
                            onClick = { onSelect(mode) },
                            role = Role.RadioButton,
                        )
                        .padding(vertical = 4.dp),
            ) {
                RadioButton(selected = mode == selected, onClick = null)
                Spacer(Modifier.width(12.dp))
                Text(stringResource(mode.label))
            }
        }
    }
}

private val ChipMode.label: Int
    get() =
        when (this) {
            ChipMode.TEXT -> R.string.chip_mode_text
            ChipMode.ICON -> R.string.chip_mode_icon
            ChipMode.OFF -> R.string.chip_mode_off
        }

@Composable
private fun SwitchRow(
    @StringRes title: Int,
    @StringRes summary: Int,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) { SectionTitle(title, summary) }
        Spacer(Modifier.width(12.dp))
        Switch(checked = checked, onCheckedChange = onCheckedChange)
    }
}

@Composable
private fun SectionTitle(@StringRes title: Int, @StringRes summary: Int) {
    Column {
        Text(stringResource(title), style = MaterialTheme.typography.titleSmall)
        Text(
            stringResource(summary),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private fun Context.openNotificationSettings() =
    launch(
        Intent(SystemSettings.ACTION_APP_NOTIFICATION_SETTINGS)
            .putExtra(SystemSettings.EXTRA_APP_PACKAGE, packageName)
    )

/** Starts a system screen, ignoring the rare device that lacks it. */
private fun Context.launch(intent: Intent) {
    try {
        startActivity(intent)
    } catch (_: ActivityNotFoundException) {}
}
