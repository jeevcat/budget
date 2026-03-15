package com.budget.app

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import com.budget.shared.api.BudgetStatus
import com.budget.shared.api.CashFlowItem
import com.budget.shared.api.LedgerSummary
import com.budget.shared.api.PaceIndicator
import kotlin.math.abs

// -- Net summary strip -------------------------------------------------------

@Composable
fun NetSummaryStrip(ledger: LedgerSummary) {
  Row(
      modifier = Modifier.fillMaxWidth(),
      horizontalArrangement = Arrangement.spacedBy(8.dp),
  ) {
    OutlinedCard(modifier = Modifier.weight(1f)) {
      Column(
          modifier = Modifier.padding(12.dp),
          horizontalAlignment = Alignment.CenterHorizontally,
      ) {
        Text(
            text = "In",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(modifier = Modifier.height(2.dp))
        Text(
            text = formatAmount(ledger.totalIn),
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
            color = UnderBudgetColor,
        )
      }
    }
    OutlinedCard(modifier = Modifier.weight(1f)) {
      Column(
          modifier = Modifier.padding(12.dp),
          horizontalAlignment = Alignment.CenterHorizontally,
      ) {
        Text(
            text = "Net",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(modifier = Modifier.height(2.dp))
        Text(
            text = formatAmount(ledger.net, showSign = true),
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.Bold,
            color = if (ledger.net < 0) OverBudgetColor else UnderBudgetColor,
        )
      }
    }
    OutlinedCard(modifier = Modifier.weight(1f)) {
      Column(
          modifier = Modifier.padding(12.dp),
          horizontalAlignment = Alignment.CenterHorizontally,
      ) {
        Text(
            text = "Out",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(modifier = Modifier.height(2.dp))
        Text(
            text = formatAmount(ledger.totalOut),
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
            color = OverBudgetColor,
        )
      }
    }
  }
}

// -- Ledger content for LazyColumn -----------------------------------------

@OptIn(ExperimentalLayoutApi::class)
fun LazyListScope.ledgerContent(
    ledger: LedgerSummary,
    statuses: List<BudgetStatus>,
    selectedCategoryId: String?,
    onCategoryClick: (String) -> Unit,
    onCashFlowItemClick: (CashFlowItem) -> Unit,
    onMonthlyBudgetsClick: (() -> Unit)? = null,
) {
  // Net summary strip
  item(key = "ledger-net-summary") { NetSummaryStrip(ledger = ledger) }

  // IN section
  if (ledger.income.isNotEmpty()) {
    item(key = "ledger-in-label") { LedgerSectionLabel(text = "IN") }
    items(ledger.income, key = { "ledger-in-${it.categoryId ?: it.label}" }) { item ->
      LedgerIncomeRow(item = item, onClick = { onCashFlowItemClick(item) })
    }
    item(key = "ledger-in-subtotal") {
      LedgerSubtotalRow(
          label = "Total In",
          amount = formatAmount(ledger.totalIn, showSign = true),
          amountColor = UnderBudgetColor,
      )
    }
  }

  // OUT section
  item(key = "ledger-out-label") { LedgerSectionLabel(text = "OUT") }

  // Column headers
  item(key = "ledger-col-headers") { LedgerColumnHeaders() }

  val withSpend = statuses.filter { it.spent != 0.0 }
  val zeroSpend = statuses.filter { it.spent == 0.0 }

  items(withSpend, key = { "ledger-out-${it.categoryId}" }) { status ->
    LedgerBudgetRow(
        status = status,
        barMax = ledger.barMax,
        selected = status.categoryId == selectedCategoryId,
        onClick = { onCategoryClick(status.categoryId) },
    )
  }

  // Zero-spend chips
  if (zeroSpend.isNotEmpty()) {
    item(key = "ledger-zero-chips") {
      FlowRow(
          modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
          horizontalArrangement = Arrangement.spacedBy(6.dp),
          verticalArrangement = Arrangement.spacedBy(6.dp),
      ) {
        zeroSpend.forEach { status ->
          val color = paceColor(status.pace)
          val isSelected = status.categoryId == selectedCategoryId
          FilterChip(
              selected = isSelected,
              onClick = { onCategoryClick(status.categoryId) },
              label = {
                Text(
                    text = status.categoryName,
                    style = MaterialTheme.typography.labelSmall,
                )
              },
              leadingIcon = {
                Canvas(modifier = Modifier.size(6.dp).clip(CircleShape)) {
                  drawCircle(color = color)
                }
              },
              colors =
                  FilterChipDefaults.filterChipColors(
                      selectedContainerColor = MaterialTheme.colorScheme.secondaryContainer,
                  ),
          )
        }
      }
    }
  }

  if (ledger.unbudgeted.isNotEmpty()) {
    item(key = "ledger-unbudgeted-divider") {
      HorizontalDivider(
          modifier = Modifier.padding(vertical = 4.dp),
          color = MaterialTheme.colorScheme.outlineVariant,
      )
    }
    items(ledger.unbudgeted, key = { "ledger-ub-${it.categoryId ?: it.label}" }) { item ->
      LedgerUnbudgetedRow(item = item, onClick = { onCashFlowItemClick(item) })
    }
  }

  // Monthly budgets row (annual ledger only)
  if (onMonthlyBudgetsClick != null && ledger.monthlySpent > 0) {
    item(key = "ledger-monthly-budgets") {
      HorizontalDivider(
          modifier = Modifier.padding(vertical = 4.dp),
          color = MaterialTheme.colorScheme.outlineVariant,
      )
      LedgerMonthlyBudgetsRow(
          ledger = ledger,
          onClick = onMonthlyBudgetsClick,
      )
    }
  }

  item(key = "ledger-out-subtotal") {
    LedgerSubtotalRow(
        label = "Total Out",
        amount = formatAmount(ledger.totalOut),
        amountColor = OverBudgetColor,
    )
  }

  // NET section
  item(key = "ledger-net") {
    LedgerNetRow(
        net = ledger.net,
        saved = ledger.saved,
        hasIncome = ledger.income.isNotEmpty(),
    )
  }
}

// -- Section label ---------------------------------------------------------

@Composable
private fun LedgerSectionLabel(text: String) {
  Text(
      text = text,
      style = MaterialTheme.typography.labelMedium,
      color = MaterialTheme.colorScheme.onSurfaceVariant,
      modifier = Modifier.padding(top = 8.dp, bottom = 2.dp),
  )
}

// -- Column headers --------------------------------------------------------

@Composable
private fun LedgerColumnHeaders() {
  Row(
      modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp, horizontal = 4.dp),
      verticalAlignment = Alignment.CenterVertically,
      horizontalArrangement = Arrangement.spacedBy(8.dp),
  ) {
    // Pace dot spacer
    Spacer(modifier = Modifier.size(8.dp))

    Text(
        text = "Name",
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.weight(1f),
    )

    // Bar spacer
    Spacer(modifier = Modifier.widthIn(min = 48.dp, max = 80.dp))

    Text(
        text = "Budget",
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.End,
    )

    Text(
        text = "Spent",
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.End,
    )

    Text(
        text = "Δ",
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.End,
    )
  }
  HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
}

// -- Income row ------------------------------------------------------------

@Composable
private fun LedgerIncomeRow(item: CashFlowItem, onClick: () -> Unit) {
  Row(
      modifier = Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 6.dp),
      horizontalArrangement = Arrangement.SpaceBetween,
      verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
        text = item.label,
        style = MaterialTheme.typography.bodyMedium,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.weight(1f),
    )
    Text(
        text = formatAmount(item.amount, showSign = true),
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Medium,
        color = UnderBudgetColor,
    )
  }
}

// -- Budget row (with pace dot + inline bar) -------------------------------

@Composable
private fun LedgerBudgetRow(
    status: BudgetStatus,
    barMax: Double,
    selected: Boolean,
    onClick: () -> Unit,
) {
  val pace = status.pace
  val color = paceColor(pace)
  val isOverBudget = pace == PaceIndicator.OVER_BUDGET
  val bgModifier =
      if (isOverBudget) {
        Modifier.background(OverBudgetColor.copy(alpha = 0.08f))
      } else {
        Modifier
      }
  val containerModifier =
      if (selected) {
        Modifier.background(MaterialTheme.colorScheme.secondaryContainer)
      } else {
        bgModifier
      }

  Row(
      modifier =
          containerModifier
              .fillMaxWidth()
              .clickable(onClick = onClick)
              .padding(vertical = 6.dp, horizontal = 4.dp),
      verticalAlignment = Alignment.CenterVertically,
      horizontalArrangement = Arrangement.spacedBy(8.dp),
  ) {
    // Pace dot
    Canvas(modifier = Modifier.size(8.dp).clip(CircleShape)) { drawCircle(color = color) }

    // Category name
    Text(
        text = status.categoryName,
        style = MaterialTheme.typography.bodyMedium,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.weight(1f),
    )

    // Inline spend bar
    InlineSpendBar(
        spent = status.spent,
        budget = status.budgetAmount,
        barMax = barMax,
        pace = pace,
    )

    // Budget amount
    Text(
        text = formatAmount(status.budgetAmount),
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )

    // Spent amount
    Text(
        text = formatAmount(status.spent),
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Medium,
    )

    // Delta (remaining)
    Text(
        text = formatAmount(status.remaining, showSign = true),
        style = MaterialTheme.typography.labelSmall,
        fontWeight = FontWeight.Bold,
        color = color,
    )
  }
}

// -- Monthly budgets row (annual ledger) -----------------------------------

@Composable
private fun LedgerMonthlyBudgetsRow(
    ledger: LedgerSummary,
    onClick: () -> Unit,
) {
  Row(
      modifier =
          Modifier.fillMaxWidth()
              .clickable(onClick = onClick)
              .padding(vertical = 6.dp, horizontal = 4.dp),
      verticalAlignment = Alignment.CenterVertically,
      horizontalArrangement = Arrangement.spacedBy(8.dp),
  ) {
    Text(
        text = "Monthly budgets",
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.weight(1f),
    )
    Text(
        text = formatAmount(ledger.monthlyBudget),
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Text(
        text = formatAmount(ledger.monthlySpent),
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Medium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Text(
        text = formatAmount(ledger.monthlyRemaining, showSign = true),
        style = MaterialTheme.typography.labelSmall,
        fontWeight = FontWeight.Bold,
        color =
            if (ledger.monthlyRemaining < 0) OverBudgetColor
            else MaterialTheme.colorScheme.onSurfaceVariant,
    )
  }
}

// -- Unbudgeted row --------------------------------------------------------

@Composable
private fun LedgerUnbudgetedRow(item: CashFlowItem, onClick: () -> Unit) {
  Row(
      modifier = Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 6.dp),
      horizontalArrangement = Arrangement.SpaceBetween,
      verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
        text = item.label,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.weight(1f),
    )
    Text(
        text = formatAmount(item.amount),
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
  }
}

// -- Subtotal row ----------------------------------------------------------

@Composable
private fun LedgerSubtotalRow(label: String, amount: String, amountColor: Color) {
  HorizontalDivider(
      modifier = Modifier.padding(top = 6.dp),
      color = MaterialTheme.colorScheme.outlineVariant,
  )
  Row(
      modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
      horizontalArrangement = Arrangement.SpaceBetween,
      verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
        text = label,
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Bold,
    )
    Text(
        text = amount,
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Bold,
        color = amountColor,
    )
  }
}

// -- Net row ---------------------------------------------------------------

@Composable
private fun LedgerNetRow(net: Double, saved: Double, hasIncome: Boolean) {
  HorizontalDivider(
      modifier = Modifier.padding(top = 4.dp),
      thickness = 2.dp,
      color = MaterialTheme.colorScheme.outline,
  )
  Column(modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
      Text(
          text = "Net",
          style = MaterialTheme.typography.bodyLarge,
          fontWeight = FontWeight.Bold,
      )
      Text(
          text = formatAmount(net, showSign = true),
          style = MaterialTheme.typography.bodyLarge,
          fontWeight = FontWeight.Bold,
          color = if (net < 0) OverBudgetColor else UnderBudgetColor,
      )
    }
    if (hasIncome) {
      Spacer(modifier = Modifier.height(2.dp))
      Row(
          modifier = Modifier.fillMaxWidth(),
          horizontalArrangement = Arrangement.SpaceBetween,
          verticalAlignment = Alignment.CenterVertically,
      ) {
        Text(
            text = "Saved",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = formatAmount(saved, showSign = true),
            style = MaterialTheme.typography.bodySmall,
            fontWeight = FontWeight.Medium,
            color = if (saved < 0) OverBudgetColor else UnderBudgetColor,
        )
      }
    }
  }
}

// -- Inline spend bar ------------------------------------------------------

@Composable
private fun InlineSpendBar(
    spent: Double,
    budget: Double,
    barMax: Double,
    pace: PaceIndicator,
) {
  val fillColor = paceColor(pace)
  val trackColor = MaterialTheme.colorScheme.surfaceVariant
  val markColor = MaterialTheme.colorScheme.outline

  Canvas(modifier = Modifier.widthIn(min = 48.dp, max = 80.dp).height(8.dp)) {
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
