package com.budget.app

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.budget.shared.api.ProjectChildSpending
import com.budget.shared.api.ProjectStatusEntry
import com.budget.shared.api.TransactionEntry

@Composable
fun ProjectDrillDownContent(
    project: ProjectStatusEntry,
    transactions: List<TransactionEntry>,
    modifier: Modifier = Modifier,
    onTransactionClick: ((String) -> Unit)? = null,
    onBack: () -> Unit,
) {
  val totalSpent = project.children.sumOf { it.spent }
  val color = paceColor(project.pace)

  LazyColumn(
      modifier = modifier.fillMaxSize(),
      contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 24.dp),
      verticalArrangement = Arrangement.spacedBy(12.dp),
  ) {
    item(key = "drilldown-nav") { DrillDownBreadcrumb(project.categoryName, onBack) }
    item(key = "drilldown-summary") { DrillDownSummaryCards(project, totalSpent, color) }
    drillDownChildrenSection(project, totalSpent, color)
    drillDownTransactionsSection(transactions, onTransactionClick)
  }
}

@Composable
private fun DrillDownBreadcrumb(categoryName: String, onBack: () -> Unit) {
  TextButton(onClick = onBack) {
    Icon(
        imageVector = Icons.AutoMirrored.Filled.ArrowBack,
        contentDescription = "Back to projects",
        modifier = Modifier.size(18.dp),
    )
    Spacer(modifier = Modifier.width(4.dp))
    Text(text = "All Projects", style = MaterialTheme.typography.bodyMedium)
    Text(
        text = " › ",
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Text(
        text = categoryName,
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.SemiBold,
    )
  }
}

@Composable
private fun DrillDownSummaryCards(
    project: ProjectStatusEntry,
    totalSpent: Double,
    color: Color,
) {
  Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
      StatCard("Project Budget", formatAmount(project.budgetAmount), Modifier.weight(1f))
      StatCard("Spent", formatAmount(totalSpent), Modifier.weight(1f))
    }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
      StatCard(
          label = "Remaining",
          value = formatAmount(project.remaining),
          modifier = Modifier.weight(1f),
          valueColor = if (project.remaining < 0) OverBudgetColor else null,
      )
      StatCard("Status", paceLabel(project.pace), Modifier.weight(1f), valueColor = color)
    }
  }
}

private fun LazyListScope.drillDownChildrenSection(
    project: ProjectStatusEntry,
    totalSpent: Double,
    color: Color,
) {
  if (project.children.isEmpty()) {
    item(key = "drilldown-empty") {
      Text(
          text = "No spending yet.",
          style = MaterialTheme.typography.bodyMedium,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
      )
    }
    return
  }
  item(key = "drilldown-distribution") {
    SpendingDistributionCard(project.children, totalSpent, color)
  }
  item(key = "drilldown-breakdown") {
    SubCategoryBreakdownCard(project.children, totalSpent, color)
  }
}

@Composable
private fun SpendingDistributionCard(
    children: List<ProjectChildSpending>,
    totalSpent: Double,
    color: Color,
) {
  ElevatedCard(modifier = Modifier.fillMaxWidth()) {
    Column(modifier = Modifier.padding(16.dp)) {
      Text(
          "Spending Distribution",
          style = MaterialTheme.typography.titleMedium,
          fontWeight = FontWeight.Bold,
      )
      Spacer(modifier = Modifier.height(12.dp))
      children.forEach { child ->
        val pct = if (totalSpent > 0) child.spent / totalSpent else 0.0
        Column(modifier = Modifier.padding(vertical = 4.dp)) {
          Row(
              modifier = Modifier.fillMaxWidth(),
              horizontalArrangement = Arrangement.SpaceBetween,
          ) {
            Text(
                child.categoryName,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Text(
                formatAmount(child.spent),
                style = MaterialTheme.typography.bodySmall,
                fontWeight = FontWeight.Medium,
            )
          }
          Spacer(modifier = Modifier.height(4.dp))
          DistributionBar(fraction = pct, color = color)
        }
      }
    }
  }
}

@Composable
private fun SubCategoryBreakdownCard(
    children: List<ProjectChildSpending>,
    totalSpent: Double,
    color: Color,
) {
  ElevatedCard(modifier = Modifier.fillMaxWidth()) {
    Column(modifier = Modifier.padding(16.dp)) {
      Text(
          "Sub-Category Breakdown",
          style = MaterialTheme.typography.titleMedium,
          fontWeight = FontWeight.Bold,
      )
      Spacer(modifier = Modifier.height(12.dp))
      children.forEach { child ->
        val pct = if (totalSpent > 0) (child.spent / totalSpent * 100).toInt() else 0
        Row(
            modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
          Box(
              modifier =
                  Modifier.size(40.dp).clip(CircleShape).background(color.copy(alpha = 0.15f)),
              contentAlignment = Alignment.Center,
          ) {
            Text(
                "$pct%",
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Bold,
                color = color,
            )
          }
          Spacer(modifier = Modifier.width(12.dp))
          Column(modifier = Modifier.weight(1f)) {
            Text(
                child.categoryName,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.Medium,
            )
            Text(
                text = "${formatAmount(child.spent)} of ${formatAmount(totalSpent)} total",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
          }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
      }
    }
  }
}

private fun LazyListScope.drillDownTransactionsSection(
    transactions: List<TransactionEntry>,
    onTransactionClick: ((String) -> Unit)?,
) {
  item(key = "drilldown-txn-header") {
    Row(
        modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
      Text(
          "Transactions",
          style = MaterialTheme.typography.titleMedium,
          fontWeight = FontWeight.Bold,
      )
      Text(
          "${transactions.size}",
          style = MaterialTheme.typography.labelMedium,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
      )
    }
  }
  if (transactions.isEmpty()) {
    item(key = "drilldown-txn-empty") {
      Text(
          "No transactions yet",
          style = MaterialTheme.typography.bodyMedium,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
      )
    }
  } else {
    items(transactions, key = { it.id }) { txn ->
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
private fun DistributionBar(
    fraction: Double,
    color: Color,
) {
  val trackColor = MaterialTheme.colorScheme.surfaceVariant
  Canvas(
      modifier = Modifier.fillMaxWidth().height(6.dp).clip(RoundedCornerShape(3.dp)),
  ) {
    val w = size.width
    val h = size.height
    val radius = CornerRadius(h / 2, h / 2)
    drawRoundRect(color = trackColor, size = Size(w, h), cornerRadius = radius)
    val fillW = (w * fraction.coerceIn(0.0, 1.0)).toFloat()
    if (fillW > 0) {
      drawRoundRect(
          color = color.copy(alpha = 0.8f),
          size = Size(fillW, h),
          cornerRadius = radius,
      )
    }
  }
}
