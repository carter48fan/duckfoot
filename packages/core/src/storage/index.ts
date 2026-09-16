import type { StorageDriver } from '../types';
import { OpfsStorageDriver } from './opfs';
import { MemoryStorageDriver } from './memory';

let globalDriver: StorageDriver | null = null;

export function getStorageDriver(): StorageDriver {
  if (globalDriver) return globalDriver;

  if (typeof navigator !== 'undefined' && navigator.storage && navigator.storage.getDirectory) {
    globalDriver = new OpfsStorageDriver();
  } else {
    globalDriver = new MemoryStorageDriver();
  }

  return globalDriver;
}

export function setStorageDriver(driver: StorageDriver) {
  globalDriver = driver;
}
