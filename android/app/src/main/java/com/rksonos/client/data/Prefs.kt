package com.rksonos.client.data

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map

private val Context.dataStore by preferencesDataStore("rksonos")

class Prefs(private val context: Context) {
    private val keyLast = stringPreferencesKey("last_server")

    suspend fun loadLastServer(): String? =
        context.dataStore.data.map { it[keyLast] }.first()

    suspend fun saveLastServer(url: String) {
        context.dataStore.edit { it[keyLast] = url.trim().trimEnd('/') }
    }
}
