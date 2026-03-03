package com.budget.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.budget.shared.api.Transaction
import com.budget.shared.viewmodel.DisplayCategory
import com.budget.shared.viewmodel.TransactionsUiState
import com.budget.shared.viewmodel.TransactionsViewModel

// -- Root screen -----------------------------------------------------------

@Composable
fun TransactionsScreen(
    viewModel: TransactionsViewModel,
    onTransactionClick: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()

  Box(modifier = modifier.fillMaxSize()) {
    when {
      state.loading && state.transactions.isEmpty() -> {
        CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
      }
      state.error != null && state.transactions.isEmpty() -> {
        Column(
            modifier = Modifier.align(Alignment.Center).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
          Text(
              text = state.error ?: "Unknown error",
              color = MaterialTheme.colorScheme.error,
          )
          Spacer(modifier = Modifier.height(12.dp))
          TextButton(onClick = viewModel::refresh) { Text("Retry") }
        }
      }
      state.transactions.isEmpty() -> {
        Column(
            modifier = Modifier.align(Alignment.Center).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
          Text(
              text = "All caught up!",
              style = MaterialTheme.typography.headlineSmall,
          )
          Spacer(modifier = Modifier.height(4.dp))
          Text(
              text = "No uncategorized transactions.",
              style = MaterialTheme.typography.bodyMedium,
              color = MaterialTheme.colorScheme.onSurfaceVariant,
          )
        }
      }
      else -> {
        TransactionList(
            state = state,
            onSelect = { onTransactionClick(it.id) },
        )
      }
    }
  }
}

// -- Transaction list ------------------------------------------------------

@Composable
private fun TransactionList(
    state: TransactionsUiState,
    onSelect: (Transaction) -> Unit,
) {
  val categoryMap = state.categories.associateBy { it.id }

  LazyColumn(
      modifier = Modifier.fillMaxSize(),
      contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 24.dp),
      verticalArrangement = Arrangement.spacedBy(2.dp),
  ) {
    item {
      Text(
          text = "${state.total} uncategorized",
          style = MaterialTheme.typography.labelLarge,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
          modifier = Modifier.padding(bottom = 8.dp),
      )
    }

    if (state.categorizing) {
      item { LinearProgressIndicator(modifier = Modifier.fillMaxWidth()) }
    }

    items(state.transactions, key = { it.id }) { txn ->
      val catName = resolveCategoryName(txn, categoryMap)
      TransactionRow(
          merchant = txn.merchantName.ifEmpty { txn.remittanceInformation.firstOrNull() ?: "" },
          date = formatShortDate(txn.postedDate),
          amount = formatTransactionAmount(txn.amount),
          categoryName = catName,
          suggestion = txn.suggestedCategory,
          onClick = { onSelect(txn) },
      )
    }
  }
}

private fun resolveCategoryName(
    txn: Transaction,
    categoryMap: Map<String, DisplayCategory>,
): String? {
  if (txn.categoryMethod == null) return null
  return txn.categoryId?.let { categoryMap[it]?.displayName }
}
