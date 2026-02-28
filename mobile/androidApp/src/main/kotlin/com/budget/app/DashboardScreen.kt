package com.budget.app

import androidx.activity.compose.BackHandler
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetMonth
import com.budget.shared.api.BudgetStatus
import com.budget.shared.api.PaceIndicator
import com.budget.shared.api.ProjectStatusEntry
import com.budget.shared.api.TransactionEntry
import com.budget.shared.config.ServerConfig
import com.budget.shared.viewmodel.BudgetSummary
import com.budget.shared.viewmodel.DashboardUiState
import com.budget.shared.viewmodel.DashboardViewModel
import kotlin.math.abs

// -- Pace colors -----------------------------------------------------------

private val UnderBudgetColor = Color(0xFF76946A)
private val OnTrackColor = Color(0xFFDCA561)
private val OverBudgetColor = Color(0xFFC34043)

private fun paceColor(pace: PaceIndicator): Color = when (pace) {
    PaceIndicator.UNDER_BUDGET -> UnderBudgetColor
    PaceIndicator.ON_TRACK -> OnTrackColor
    PaceIndicator.OVER_BUDGET -> OverBudgetColor
}

private fun paceLabel(pace: PaceIndicator): String = when (pace) {
    PaceIndicator.UNDER_BUDGET -> "Under"
    PaceIndicator.ON_TRACK -> "On track"
    PaceIndicator.OVER_BUDGET -> "Over"
}

// -- Formatting helpers ----------------------------------------------------

private fun formatAmount(value: Double, showSign: Boolean = false): String {
    val absVal = abs(value).toLong()
    val formatted = buildString {
        val s = absVal.toString()
        for (i in s.indices) {
            if (i > 0 && (s.length - i) % 3 == 0) append(',')
            append(s[i])
        }
    }
    val prefix = when {
        showSign && value > 0 -> "+"
        value < 0 -> "-"
        showSign && value < 0 -> "-"
        else -> ""
    }
    return "${prefix}€$formatted"
}

private fun formatMonthRange(month: BudgetMonth): String {
    // startDate format: "2026-02-28"
    val parts = month.startDate.split("-")
    if (parts.size < 3) return month.startDate
    val monthNames = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")
    val m = parts[1].toIntOrNull() ?: return month.startDate
    val d = parts[2].toIntOrNull() ?: return month.startDate
    val y = parts[0]
    val start = "${monthNames[m - 1]} $d"

    val end = month.endDate
    if (end == null) return "$start, $y"
    val ep = end.split("-")
    if (ep.size < 3) return "$start – $end"
    val em = ep[1].toIntOrNull() ?: return "$start – $end"
    val ed = ep[2].toIntOrNull() ?: return "$start – $end"
    val ey = ep[0]
    val endStr = "${monthNames[em - 1]} $ed"
    return if (y == ey) "$start – $endStr, $y" else "$start, $y – $endStr, $ey"
}

// -- Root screen -----------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DashboardScreen(config: ServerConfig, onLogout: () -> Unit) {
    val viewModel: DashboardViewModel = viewModel {
        DashboardViewModel(config.serverUrl, config.apiKey)
    }
    val state by viewModel.uiState.collectAsStateWithLifecycle()

    val selectedCategoryId = state.selectedCategoryId
    if (selectedCategoryId != null) {
        BackHandler { viewModel.selectCategory(null) }
        CategoryTransactionsScreen(
            state = state,
            categoryId = selectedCategoryId,
            onBack = { viewModel.selectCategory(null) },
        )
        return
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Budget") },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
                actions = {
                    IconButton(onClick = onLogout) {
                        Icon(
                            Icons.AutoMirrored.Filled.Logout,
                            contentDescription = "Disconnect",
                        )
                    }
                },
            )
        },
    ) { innerPadding ->
        Box(modifier = Modifier.padding(innerPadding).fillMaxSize()) {
            when {
                state.loading && state.currentMonth == null -> {
                    CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
                }
                state.error != null && state.currentMonth == null -> {
                    Text(
                        text = state.error ?: "Unknown error",
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.align(Alignment.Center).padding(24.dp),
                    )
                }
                else -> {
                    DashboardContent(state = state, viewModel = viewModel)
                }
            }
            // Subtle loading bar when refreshing (data already visible)
            if (state.loading && state.currentMonth != null) {
                LinearProgressIndicator(
                    modifier = Modifier.fillMaxWidth().align(Alignment.TopCenter),
                )
            }
        }
    }
}

// -- Dashboard content with tabs -------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DashboardContent(state: DashboardUiState, viewModel: DashboardViewModel) {
    val tabs = buildList {
        add(BudgetMode.MONTHLY)
        add(BudgetMode.ANNUAL)
        if (state.projects.isNotEmpty()) add(BudgetMode.PROJECT)
    }
    val selectedIndex = tabs.indexOf(state.selectedTab).coerceAtLeast(0)

    Column(modifier = Modifier.fillMaxSize()) {
        PrimaryTabRow(selectedTabIndex = selectedIndex) {
            tabs.forEachIndexed { index, mode ->
                Tab(
                    selected = index == selectedIndex,
                    onClick = { viewModel.selectTab(mode) },
                    text = {
                        Text(
                            when (mode) {
                                BudgetMode.MONTHLY -> "Monthly"
                                BudgetMode.ANNUAL -> "Annual"
                                BudgetMode.PROJECT -> "Projects"
                            }
                        )
                    },
                )
            }
        }

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(
                start = 16.dp, end = 16.dp, top = 12.dp, bottom = 24.dp
            ),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            when (state.selectedTab) {
                BudgetMode.MONTHLY -> {
                    item {
                        MonthNavigator(
                            month = state.currentMonth,
                            timeLabel = state.monthlyTimeLabel,
                            isCurrentMonth = state.isCurrentMonth,
                            hasPrev = state.hasPrevMonth,
                            hasNext = state.hasNextMonth,
                            uncategorizedCount = state.uncategorizedCount,
                            onPrev = viewModel::goToPreviousMonth,
                            onNext = viewModel::goToNextMonth,
                        )
                    }
                    item { SummaryCards(summary = state.monthlySummary) }
                    items(state.monthlyStatuses, key = { it.categoryId }) { status ->
                        CategoryRow(
                            name = status.categoryName,
                            spent = status.spent,
                            budget = status.budgetAmount,
                            remaining = status.remaining,
                            pace = status.pace,
                            barMax = state.monthlySummary.barMax,
                            selected = false,
                            onClick = { viewModel.selectCategory(status.categoryId) },
                        )
                    }
                    transactionSection(transactions = state.monthlyTransactions)
                }
                BudgetMode.ANNUAL -> {
                    item {
                        AnnualHeader(
                            budgetYear = state.budgetYear,
                            timeLabel = state.annualTimeLabel,
                        )
                    }
                    item { SummaryCards(summary = state.annualSummary) }
                    items(state.annualStatuses, key = { it.categoryId }) { status ->
                        CategoryRow(
                            name = status.categoryName,
                            spent = status.spent,
                            budget = status.budgetAmount,
                            remaining = status.remaining,
                            pace = status.pace,
                            barMax = state.annualSummary.barMax,
                            selected = false,
                            onClick = { viewModel.selectCategory(status.categoryId) },
                        )
                    }
                    transactionSection(transactions = state.annualTransactions)
                }
                BudgetMode.PROJECT -> {
                    item { SummaryCards(summary = state.projectSummary) }
                    items(state.projects, key = { it.categoryId }) { project ->
                        CategoryRow(
                            name = project.categoryName,
                            spent = project.spent,
                            budget = project.budgetAmount,
                            remaining = project.remaining,
                            pace = project.pace,
                            barMax = state.projectSummary.barMax,
                            selected = false,
                            onClick = { viewModel.selectCategory(project.categoryId) },
                        )
                    }
                    transactionSection(transactions = state.projectTransactions)
                }
            }
        }
    }
}

// -- Month navigator -------------------------------------------------------

@Composable
private fun MonthNavigator(
    month: BudgetMonth?,
    timeLabel: String,
    isCurrentMonth: Boolean,
    hasPrev: Boolean,
    hasNext: Boolean,
    uncategorizedCount: Int,
    onPrev: () -> Unit,
    onNext: () -> Unit,
) {
    if (month == null) return
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = onPrev, enabled = hasPrev) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Previous month")
        }
        Column(
            modifier = Modifier.weight(1f),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = formatMonthRange(month),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
            )
            Text(
                text = if (isCurrentMonth) timeLabel else "Closed",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        IconButton(onClick = onNext, enabled = hasNext) {
            Icon(Icons.AutoMirrored.Filled.ArrowForward, contentDescription = "Next month")
        }
        if (uncategorizedCount > 0) {
            Spacer(modifier = Modifier.width(4.dp))
            Text(
                text = "$uncategorizedCount uncategorized",
                style = MaterialTheme.typography.labelSmall,
                color = OnTrackColor,
            )
        }
    }
}

// -- Annual header ---------------------------------------------------------

@Composable
private fun AnnualHeader(budgetYear: Int, timeLabel: String) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = budgetYear.toString(),
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
        )
        if (timeLabel.isNotEmpty()) {
            Text(
                text = timeLabel,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// -- Summary cards (2×2) ---------------------------------------------------

@Composable
private fun SummaryCards(summary: BudgetSummary) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            StatCard(
                label = "Budget",
                value = formatAmount(summary.totalBudget),
                modifier = Modifier.weight(1f),
            )
            StatCard(
                label = "Spent",
                value = formatAmount(summary.totalSpent),
                modifier = Modifier.weight(1f),
            )
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            StatCard(
                label = "Remaining",
                value = formatAmount(summary.totalRemaining),
                valueColor = if (summary.totalRemaining < 0) OverBudgetColor else null,
                modifier = Modifier.weight(1f),
            )
            StatCard(
                label = "Categories",
                value = if (summary.overBudgetCount > 0) "${summary.overBudgetCount} over" else "All on track",
                valueColor = if (summary.overBudgetCount > 0) OverBudgetColor else UnderBudgetColor,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun StatCard(
    label: String,
    value: String,
    modifier: Modifier = Modifier,
    valueColor: Color? = null,
) {
    ElevatedCard(modifier = modifier) {
        Column(modifier = Modifier.padding(12.dp)) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                text = value,
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.Bold,
                color = valueColor ?: MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}

// -- Unified category row (bar + details) ----------------------------------

@Composable
private fun CategoryRow(
    name: String,
    spent: Double,
    budget: Double,
    remaining: Double,
    pace: PaceIndicator,
    barMax: Double,
    selected: Boolean,
    onClick: () -> Unit,
) {
    val color = paceColor(pace)
    val containerColor = if (selected) {
        MaterialTheme.colorScheme.secondaryContainer
    } else {
        MaterialTheme.colorScheme.surface
    }

    ElevatedCard(
        modifier = Modifier
            .fillMaxWidth()
            .animateContentSize(),
        colors = androidx.compose.material3.CardDefaults.elevatedCardColors(
            containerColor = containerColor,
        ),
    ) {
        Column(
            modifier = Modifier
                .clickable(onClick = onClick)
                .padding(12.dp),
        ) {
            // Top line: name + spent amount
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = name,
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    text = formatAmount(spent),
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.Bold,
                )
            }

            Spacer(modifier = Modifier.height(6.dp))

            // Spend bar
            SpendBar(
                spent = spent,
                budget = budget,
                barMax = barMax,
                pace = pace,
            )

            Spacer(modifier = Modifier.height(6.dp))

            // Bottom line: budget info + pace + remaining
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        text = if (budget > 0) "of ${formatAmount(budget)}" else "no budget",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    PaceBadge(pace = pace)
                    Text(
                        text = formatAmount(remaining, showSign = true),
                        style = MaterialTheme.typography.labelMedium,
                        fontWeight = FontWeight.Bold,
                        color = color,
                    )
                }
            }
        }
    }
}

// -- Spend bar (horizontal) ------------------------------------------------

@Composable
private fun SpendBar(
    spent: Double,
    budget: Double,
    barMax: Double,
    pace: PaceIndicator,
) {
    val fillColor = paceColor(pace)
    val trackColor = MaterialTheme.colorScheme.surfaceVariant
    val markColor = MaterialTheme.colorScheme.outline

    Canvas(
        modifier = Modifier
            .fillMaxWidth()
            .height(10.dp)
            .clip(RoundedCornerShape(5.dp)),
    ) {
        val w = size.width
        val h = size.height
        val radius = CornerRadius(h / 2, h / 2)

        // Track background
        drawRoundRect(
            color = trackColor,
            size = Size(w, h),
            cornerRadius = radius,
        )

        // Fill bar
        val fillFraction = if (barMax > 0) (abs(spent) / barMax).coerceIn(0.0, 1.0) else 0.0
        if (fillFraction > 0) {
            drawRoundRect(
                color = fillColor.copy(alpha = 0.8f),
                size = Size((w * fillFraction).toFloat(), h),
                cornerRadius = radius,
            )
        }

        // Budget mark (vertical line)
        if (budget > 0 && barMax > 0) {
            val markX = ((budget / barMax) * w).toFloat().coerceIn(0f, w)
            drawLine(
                color = markColor,
                start = Offset(markX, 0f),
                end = Offset(markX, h),
                strokeWidth = 2f,
                cap = StrokeCap.Round,
            )
        }
    }
}

// -- Pace badge ------------------------------------------------------------

@Composable
private fun PaceBadge(pace: PaceIndicator) {
    val color = paceColor(pace)
    Text(
        text = paceLabel(pace),
        style = MaterialTheme.typography.labelSmall,
        color = color,
        modifier = Modifier
            .clip(RoundedCornerShape(4.dp))
            .background(color.copy(alpha = 0.15f))
            .padding(horizontal = 6.dp, vertical = 2.dp),
    )
}

// -- Transaction section ---------------------------------------------------

private fun androidx.compose.foundation.lazy.LazyListScope.transactionSection(
    transactions: List<TransactionEntry>,
) {
    if (transactions.isEmpty()) return

    item {
        Spacer(modifier = Modifier.height(4.dp))
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Transactions",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
            )
            Text(
                text = "${transactions.size}",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }

    items(transactions.take(50), key = { it.id }) { txn ->
        TransactionRow(txn)
    }
}

@Composable
private fun TransactionRow(txn: TransactionEntry) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = txn.merchantName.ifEmpty { txn.description },
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = formatDate(txn.postedDate),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Text(
            text = formatAmount(txn.amount),
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
            textAlign = TextAlign.End,
        )
    }
}

// -- Category transactions detail screen ------------------------------------

private data class CategoryInfo(
    val name: String,
    val spent: Double,
    val budget: Double,
    val remaining: Double,
    val pace: PaceIndicator,
    val barMax: Double,
)

private fun resolveCategoryInfo(state: DashboardUiState, categoryId: String): CategoryInfo? {
    when (state.selectedTab) {
        BudgetMode.MONTHLY -> {
            val s = state.monthlyStatuses.find { it.categoryId == categoryId }
            if (s != null) return CategoryInfo(s.categoryName, s.spent, s.budgetAmount, s.remaining, s.pace, state.monthlySummary.barMax)
        }
        BudgetMode.ANNUAL -> {
            val s = state.annualStatuses.find { it.categoryId == categoryId }
            if (s != null) return CategoryInfo(s.categoryName, s.spent, s.budgetAmount, s.remaining, s.pace, state.annualSummary.barMax)
        }
        BudgetMode.PROJECT -> {
            val p = state.projects.find { it.categoryId == categoryId }
            if (p != null) return CategoryInfo(p.categoryName, p.spent, p.budgetAmount, p.remaining, p.pace, state.projectSummary.barMax)
        }
    }
    return null
}

private fun resolveTransactions(state: DashboardUiState, categoryId: String): List<TransactionEntry> {
    val all = when (state.selectedTab) {
        BudgetMode.MONTHLY -> state.monthlyTransactions
        BudgetMode.ANNUAL -> state.annualTransactions
        BudgetMode.PROJECT -> state.projectTransactions
    }
    return all.filter { it.categoryId == categoryId }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CategoryTransactionsScreen(
    state: DashboardUiState,
    categoryId: String,
    onBack: () -> Unit,
) {
    val info = resolveCategoryInfo(state, categoryId)
    val transactions = resolveTransactions(state, categoryId)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(info?.name ?: "Category") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
            )
        },
    ) { innerPadding ->
        LazyColumn(
            modifier = Modifier.padding(innerPadding).fillMaxSize(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(
                start = 16.dp, end = 16.dp, top = 12.dp, bottom = 24.dp,
            ),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (info != null) {
                item {
                    CategoryDetailHeader(info)
                }
            }
            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = "Transactions",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Bold,
                    )
                    Text(
                        text = "${transactions.size}",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (transactions.isEmpty()) {
                item {
                    Text(
                        text = "No transactions in this category",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                items(transactions, key = { it.id }) { txn ->
                    TransactionRow(txn)
                }
            }
        }
    }
}

@Composable
private fun CategoryDetailHeader(info: CategoryInfo) {
    val color = paceColor(info.pace)
    ElevatedCard(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = formatAmount(info.spent),
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.Bold,
                )
                PaceBadge(pace = info.pace)
            }

            Spacer(modifier = Modifier.height(8.dp))

            SpendBar(
                spent = info.spent,
                budget = info.budget,
                barMax = info.barMax,
                pace = info.pace,
            )

            Spacer(modifier = Modifier.height(8.dp))

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = if (info.budget > 0) "of ${formatAmount(info.budget)}" else "no budget",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = formatAmount(info.remaining, showSign = true),
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.Bold,
                    color = color,
                )
            }
        }
    }
}

private fun formatDate(dateStr: String): String {
    val parts = dateStr.split("-")
    if (parts.size < 3) return dateStr
    val monthNames = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")
    val m = parts[1].toIntOrNull() ?: return dateStr
    val d = parts[2].toIntOrNull() ?: return dateStr
    return "${monthNames[m - 1]} $d"
}
