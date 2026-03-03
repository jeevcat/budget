package com.budget.app

import androidx.activity.compose.BackHandler
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetMonth
import com.budget.shared.api.BudgetStatus
import com.budget.shared.api.ChildCategoryInfo
import com.budget.shared.api.PaceIndicator
import com.budget.shared.api.TransactionEntry
import com.budget.shared.config.ServerConfig
import com.budget.shared.repository.DefaultBudgetRepository
import com.budget.shared.viewmodel.BudgetSummary
import com.budget.shared.viewmodel.DashboardUiState
import com.budget.shared.viewmodel.DashboardViewModel
import kotlin.math.abs

private const val UNBUDGETED_CATEGORY_ID = "__unbudgeted__"

// -- Pace colors -----------------------------------------------------------

private val PendingColor = Color(0xFF938AA9)
private val UnderBudgetColor = Color(0xFF76946A)
private val OnTrackColor = Color(0xFF7E9CD8)
private val AbovePaceColor = Color(0xFFDCA561)
private val OverBudgetColor = Color(0xFFC34043)

private fun paceColor(pace: PaceIndicator): Color =
    when (pace) {
      PaceIndicator.PENDING -> PendingColor
      PaceIndicator.UNDER_BUDGET -> UnderBudgetColor
      PaceIndicator.ON_TRACK -> OnTrackColor
      PaceIndicator.ABOVE_PACE -> AbovePaceColor
      PaceIndicator.OVER_BUDGET -> OverBudgetColor
    }

private fun paceLabel(pace: PaceIndicator, delta: Double? = null): String {
  val base =
      when (pace) {
        PaceIndicator.PENDING -> "Pending"
        PaceIndicator.UNDER_BUDGET -> "Under pace"
        PaceIndicator.ON_TRACK -> "On track"
        PaceIndicator.ABOVE_PACE -> "Above pace"
        PaceIndicator.OVER_BUDGET -> "Over budget"
      }
  val showDelta = pace != PaceIndicator.PENDING && pace != PaceIndicator.ON_TRACK
  if (delta != null && showDelta) {
    return "$base (${formatAmount(delta, showSign = true)})"
  }
  return base
}

// -- Formatting helpers ----------------------------------------------------

private fun formatMonthRange(month: BudgetMonth): String {
  val startDate = parseLocalDate(month.startDate) ?: return month.startDate
  val start = startDate.format(ShortDateFormatter)
  val y = startDate.year
  val endRaw = month.endDate ?: return "$start, $y"
  val endDate = parseLocalDate(endRaw) ?: return "$start – $endRaw"
  val endStr = endDate.format(ShortDateFormatter)
  return if (y == endDate.year) "$start – $endStr, $y" else "$start, $y – $endStr, ${endDate.year}"
}

// -- Root screen -----------------------------------------------------------

/** Standalone entry point (kept for backward compatibility). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DashboardScreen(config: ServerConfig) {
  val repository = remember(config) { DefaultBudgetRepository(config.serverUrl, config.apiKey) }
  val viewModel: DashboardViewModel = viewModel { DashboardViewModel(repository) }
  DashboardContent(viewModel = viewModel)
}

/** Content without Scaffold — used from AppShell's NavHost. */
@Composable
fun DashboardContent(
    viewModel: DashboardViewModel,
    modifier: Modifier = Modifier,
    onTransactionClick: ((String) -> Unit)? = null,
) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()

  val selectedCategoryId = state.selectedCategoryId
  if (selectedCategoryId != null) {
    BackHandler { viewModel.selectCategory(null) }
    CategoryTransactionsContent(
        state = state,
        categoryId = selectedCategoryId,
        onTransactionClick = onTransactionClick,
        modifier = modifier,
    )
    return
  }

  Box(modifier = modifier.fillMaxSize()) {
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
        DashboardTabContent(
            state = state,
            viewModel = viewModel,
        )
      }
    }
  }
}

// -- Dashboard content with tabs -------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DashboardTabContent(
    state: DashboardUiState,
    viewModel: DashboardViewModel,
) {
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
        contentPadding =
            PaddingValues(
                start = 16.dp,
                end = 16.dp,
                top = 12.dp,
                bottom = 24.dp,
            ),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
      when (state.selectedTab) {
        BudgetMode.MONTHLY -> monthlyTabContent(state = state, viewModel = viewModel)
        BudgetMode.ANNUAL -> annualTabContent(state = state, viewModel = viewModel)
        BudgetMode.PROJECT -> projectTabContent(state = state, viewModel = viewModel)
      }
    }
  }
}

// -- Tab content sections --------------------------------------------------

private fun LazyListScope.monthlyTabContent(
    state: DashboardUiState,
    viewModel: DashboardViewModel,
) {
  item {
    MonthNavigator(
        month = state.currentMonth,
        timeLabel = state.monthlyTimeLabel,
        isCurrentMonth = state.isCurrentMonth,
        hasPrev = state.hasPrevMonth,
        hasNext = state.hasNextMonth,
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
        paceDelta = status.paceDelta,
        barMax = state.monthlySummary.barMax,
        selected = false,
        onClick = { viewModel.selectCategory(status.categoryId) },
    )
  }
  if (state.unbudgetedSpent > 0) {
    item {
      UnbudgetedRow(
          spent = state.unbudgetedSpent,
          count = state.unbudgetedTransactions.size,
          onClick = { viewModel.selectCategory(UNBUDGETED_CATEGORY_ID) },
      )
    }
  }
}

private fun LazyListScope.annualTabContent(
    state: DashboardUiState,
    viewModel: DashboardViewModel,
) {
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
        paceDelta = status.paceDelta,
        barMax = state.annualSummary.barMax,
        selected = false,
        onClick = { viewModel.selectCategory(status.categoryId) },
    )
  }
}

private fun LazyListScope.projectTabContent(
    state: DashboardUiState,
    viewModel: DashboardViewModel,
) {
  item { SummaryCards(summary = state.projectSummary) }
  items(state.projects, key = { it.categoryId }) { project ->
    CategoryRow(
        name = project.categoryName,
        spent = project.spent,
        budget = project.budgetAmount,
        remaining = project.remaining,
        pace = project.pace,
        paceDelta = project.paceDelta,
        barMax = state.projectSummary.barMax,
        selected = false,
        onClick = { viewModel.selectCategory(project.categoryId) },
    )
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

// -- Unbudgeted row --------------------------------------------------------

@Composable
private fun UnbudgetedRow(spent: Double, count: Int, onClick: () -> Unit) {
  ElevatedCard(modifier = Modifier.fillMaxWidth()) {
    Row(
        modifier = Modifier.clickable(onClick = onClick).padding(12.dp).fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
      Column {
        Text(
            text = "Unbudgeted",
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = FontWeight.Medium,
        )
        Text(
            text = "$count transaction${if (count != 1) "s" else ""}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
      }
      Text(
          text = formatAmount(spent),
          style = MaterialTheme.typography.bodyLarge,
          fontWeight = FontWeight.Bold,
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
          value =
              if (summary.overBudgetCount > 0) "${summary.overBudgetCount} over"
              else "All on track",
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
    paceDelta: Double,
    barMax: Double,
    selected: Boolean,
    onClick: () -> Unit,
) {
  val color = paceColor(pace)
  val containerColor =
      if (selected) {
        MaterialTheme.colorScheme.secondaryContainer
      } else {
        MaterialTheme.colorScheme.surface
      }

  ElevatedCard(
      modifier = Modifier.fillMaxWidth().animateContentSize(),
      colors =
          CardDefaults.elevatedCardColors(
              containerColor = containerColor,
          ),
  ) {
    Column(
        modifier = Modifier.clickable(onClick = onClick).padding(12.dp),
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
          PaceBadge(pace = pace, delta = paceDelta)
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
      modifier = Modifier.fillMaxWidth().height(10.dp).clip(RoundedCornerShape(5.dp)),
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
      val markX = (budget / barMax * w).toFloat().coerceIn(0f, w)
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
private fun PaceBadge(pace: PaceIndicator, delta: Double? = null) {
  val color = paceColor(pace)
  Text(
      text = paceLabel(pace, delta),
      style = MaterialTheme.typography.labelSmall,
      color = color,
      modifier =
          Modifier.clip(RoundedCornerShape(4.dp))
              .background(color.copy(alpha = 0.15f))
              .padding(horizontal = 6.dp, vertical = 2.dp),
  )
}

// -- Category name map (for badges) ----------------------------------------

private fun buildCategoryNameMap(state: DashboardUiState): Map<String, String> {
  val map = mutableMapOf<String, String>()
  for (s in state.monthlyStatuses) {
    map[s.categoryId] = s.categoryName
    for (c in s.children) map[c.categoryId] = c.categoryName
  }
  for (s in state.annualStatuses) {
    map[s.categoryId] = s.categoryName
    for (c in s.children) map[c.categoryId] = c.categoryName
  }
  for (p in state.projects) {
    map[p.categoryId] = p.categoryName
  }
  return map
}

// -- Category transactions detail screen ------------------------------------

private data class CategoryInfo(
    val name: String,
    val spent: Double,
    val budget: Double,
    val remaining: Double,
    val pace: PaceIndicator,
    val paceDelta: Double,
    val barMax: Double,
)

private fun resolveCategoryInfo(state: DashboardUiState, categoryId: String): CategoryInfo? {
  if (categoryId == UNBUDGETED_CATEGORY_ID) {
    return CategoryInfo(
        name = "Unbudgeted",
        spent = state.unbudgetedSpent,
        budget = 0.0,
        remaining = 0.0,
        pace = PaceIndicator.PENDING,
        paceDelta = 0.0,
        barMax = state.unbudgetedSpent,
    )
  }
  when (state.selectedTab) {
    BudgetMode.MONTHLY -> {
      val s = state.monthlyStatuses.find { it.categoryId == categoryId }
      if (s != null)
          return CategoryInfo(
              s.categoryName,
              s.spent,
              s.budgetAmount,
              s.remaining,
              s.pace,
              s.paceDelta,
              state.monthlySummary.barMax,
          )
    }
    BudgetMode.ANNUAL -> {
      val s = state.annualStatuses.find { it.categoryId == categoryId }
      if (s != null)
          return CategoryInfo(
              s.categoryName,
              s.spent,
              s.budgetAmount,
              s.remaining,
              s.pace,
              s.paceDelta,
              state.annualSummary.barMax,
          )
    }
    BudgetMode.PROJECT -> {
      val p = state.projects.find { it.categoryId == categoryId }
      if (p != null)
          return CategoryInfo(
              p.categoryName,
              p.spent,
              p.budgetAmount,
              p.remaining,
              p.pace,
              p.paceDelta,
              state.projectSummary.barMax,
          )
    }
  }
  return null
}

private fun resolveBudgetStatus(
    state: DashboardUiState,
    categoryId: String,
): BudgetStatus? =
    when (state.selectedTab) {
      BudgetMode.MONTHLY -> state.monthlyStatuses.find { it.categoryId == categoryId }
      BudgetMode.ANNUAL -> state.annualStatuses.find { it.categoryId == categoryId }
      BudgetMode.PROJECT -> null
    }

private fun resolveTransactions(
    state: DashboardUiState,
    categoryId: String,
): List<TransactionEntry> {
  if (categoryId == UNBUDGETED_CATEGORY_ID) {
    return state.unbudgetedTransactions
  }
  val all =
      when (state.selectedTab) {
        BudgetMode.MONTHLY -> state.monthlyTransactions
        BudgetMode.ANNUAL -> state.annualTransactions
        BudgetMode.PROJECT -> state.projectTransactions
      }
  val status = resolveBudgetStatus(state, categoryId)
  if (status != null && status.hasChildren) {
    val childIds = status.children.map { it.categoryId }.toSet() + categoryId
    return all.filter { it.categoryId in childIds }
  }
  return all.filter { it.categoryId == categoryId }
}

@Composable
private fun CategoryTransactionsContent(
    state: DashboardUiState,
    categoryId: String,
    modifier: Modifier = Modifier,
    onTransactionClick: ((String) -> Unit)? = null,
) {
  val info = resolveCategoryInfo(state, categoryId)
  val transactions = resolveTransactions(state, categoryId)
  val status = resolveBudgetStatus(state, categoryId)
  val hasChildren = status?.hasChildren == true
  val categoryNames = remember(state) { buildCategoryNameMap(state) }

  LazyColumn(
      modifier = modifier.fillMaxSize(),
      contentPadding =
          PaddingValues(
              start = 16.dp,
              end = 16.dp,
              top = 12.dp,
              bottom = 24.dp,
          ),
      verticalArrangement = Arrangement.spacedBy(12.dp),
  ) {
    if (info != null) {
      item { CategoryDetailHeader(info) }
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
    } else if (hasChildren && status != null) {
      subcategoryTransactionSections(
          transactions = transactions,
          children = status.children,
          onTransactionClick = onTransactionClick,
      )
    } else {
      items(transactions, key = { it.id }) { txn ->
        TransactionRow(
            merchant = txn.merchantName.ifEmpty { txn.remittanceInformation.firstOrNull() ?: "" },
            date = formatShortDate(txn.postedDate),
            amount = formatAmount(txn.amount),
            categoryName = txn.categoryId?.let { categoryNames[it] },
            onClick = onTransactionClick?.let { { it(txn.id) } },
        )
      }
    }
  }
}

private fun LazyListScope.subcategoryTransactionSections(
    transactions: List<TransactionEntry>,
    children: List<ChildCategoryInfo>,
    onTransactionClick: ((String) -> Unit)? = null,
) {
  val byCategory = transactions.groupBy { it.categoryId }
  val childIds = children.map { it.categoryId }.toSet()

  for (child in children) {
    val childTxns = byCategory[child.categoryId].orEmpty()
    if (childTxns.isEmpty()) continue

    item(key = "header-${child.categoryId}") {
      Row(
          modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
          horizontalArrangement = Arrangement.SpaceBetween,
          verticalAlignment = Alignment.CenterVertically,
      ) {
        Text(
            text = child.categoryName,
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            text = "${childTxns.size}",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
      }
    }
    items(childTxns, key = { it.id }) { txn ->
      TransactionRow(
          merchant = txn.merchantName.ifEmpty { txn.remittanceInformation.firstOrNull() ?: "" },
          date = formatShortDate(txn.postedDate),
          amount = formatAmount(txn.amount),
          onClick = onTransactionClick?.let { { it(txn.id) } },
      )
    }
  }

  // Transactions directly on the parent category (if any exist)
  val directTxns =
      byCategory.entries
          .filter { (catId, _) -> catId != null && catId !in childIds }
          .flatMap { it.value }
  if (directTxns.isNotEmpty()) {
    item(key = "header-direct") {
      Row(
          modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
          horizontalArrangement = Arrangement.SpaceBetween,
          verticalAlignment = Alignment.CenterVertically,
      ) {
        Text(
            text = "Other",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            text = "${directTxns.size}",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
      }
    }
    items(directTxns, key = { it.id }) { txn ->
      TransactionRow(
          merchant = txn.merchantName.ifEmpty { txn.remittanceInformation.firstOrNull() ?: "" },
          date = formatShortDate(txn.postedDate),
          amount = formatAmount(txn.amount),
          onClick = onTransactionClick?.let { { it(txn.id) } },
      )
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
        PaceBadge(pace = info.pace, delta = info.paceDelta)
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
