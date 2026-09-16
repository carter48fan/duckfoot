import React, { useState, useRef } from 'react';
import type { AssetItem, ActiveTool } from '@opencut/core';
import { PhotoStudio } from '@opencut/tool-photo';
import { AudioCutter } from '@opencut/tool-audio';
import { VideoEditor } from '@opencut/tool-video';
import { nanoid } from 'nanoid';
import {
  Image as ImageIcon,
  Music,
  Video as VideoIcon,
  Upload,
  FolderOpen,
  ChevronLeft,
  ChevronRight,
  Layers,
  Sparkles,
  Trash2
} from 'lucide-react';

export function CreativeSuite() {
  const [activeTool, setActiveTool] = useState<ActiveTool>('photo');
  const [assets, setAssets] = useState<AssetItem[]>([]);
  const [activeAssetId, setActiveAssetId] = useState<string | null>(null);
  const [isShelfOpen, setIsShelfOpen] = useState<boolean>(true);
  const [shelfFilter, setShelfFilter] = useState<'all' | 'photo' | 'audio' | 'video'>('all');

  const fileInputRef = useRef<HTMLInputElement>(null);

  // Ingest files from disk
  const handleFiles = (files: FileList | null) => {
    if (!files || files.length === 0) return;

    Array.from(files).forEach((file) => {
      let type: 'photo' | 'audio' | 'video' = 'photo';
      if (file.type.startsWith('audio/')) type = 'audio';
      else if (file.type.startsWith('video/')) type = 'video';
      else if (file.type.startsWith('image/')) type = 'photo';

      const url = URL.createObjectURL(file);
      const newItem: AssetItem = {
        id: nanoid(),
        name: file.name,
        type,
        url,
        file,
        size: file.size,
        createdAt: Date.now(),
      };

      // Extract metadata
      if (type === 'photo') {
        const img = new Image();
        img.src = url;
        img.onload = () => {
          newItem.width = img.naturalWidth;
          newItem.height = img.naturalHeight;
          setAssets((prev) => [...prev, newItem]);
          if (!activeAssetId) setActiveAssetId(newItem.id);
        };
      } else if (type === 'video' || type === 'audio') {
        const media = document.createElement(type === 'video' ? 'video' : 'audio');
        media.src = url;
        media.onloadedmetadata = () => {
          newItem.duration = media.duration;
          if (type === 'video') {
            const v = media as HTMLVideoElement;
            newItem.width = v.videoWidth;
            newItem.height = v.videoHeight;
          }
          setAssets((prev) => [...prev, newItem]);
          if (!activeAssetId) setActiveAssetId(newItem.id);
        };
      } else {
        setAssets((prev) => [...prev, newItem]);
        if (!activeAssetId) setActiveAssetId(newItem.id);
      }
    });
  };

  const handleAssetClick = (asset: AssetItem) => {
    setActiveAssetId(asset.id);
    // Automatically switch tool to match asset type if not in matching tool
    if (asset.type === 'photo' && activeTool !== 'photo') setActiveTool('photo');
    if (asset.type === 'audio' && activeTool !== 'audio' && activeTool !== 'video') setActiveTool('audio');
    if (asset.type === 'video' && activeTool !== 'video') setActiveTool('video');
  };

  const handleDeleteAsset = (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    setAssets((prev) => prev.filter((a) => a.id !== id));
    if (activeAssetId === id) setActiveAssetId(null);
  };

  const activeAsset = assets.find((a) => a.id === activeAssetId) || null;
  const filteredAssets = assets.filter((a) => (shelfFilter === 'all' ? true : a.type === shelfFilter));

  return (
    <div className="flex flex-col h-screen w-screen bg-zinc-950 text-zinc-100 overflow-hidden font-sans select-none">
      {/* Top Suite Navigation Bar */}
      <header className="h-14 border-b border-zinc-800 bg-zinc-900/90 backdrop-blur px-4 flex items-center justify-between z-30">
        {/* Brand & Shelf Toggle */}
        <div className="flex items-center gap-3">
          <button
            onClick={() => setIsShelfOpen(!isShelfOpen)}
            className="p-2 rounded-lg hover:bg-zinc-800 text-zinc-400 hover:text-white transition"
            title="Toggle Asset Shelf"
          >
            {isShelfOpen ? <ChevronLeft className="w-5 h-5" /> : <ChevronRight className="w-5 h-5" />}
          </button>
          <div className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg bg-gradient-to-tr from-indigo-600 to-violet-500 flex items-center justify-center font-black text-white text-sm shadow-md shadow-indigo-600/30">
              OC
            </div>
            <div>
              <span className="font-bold text-sm tracking-tight text-white block">OpenCut</span>
              <span className="text-[10px] text-zinc-400 font-medium -mt-1 block">Creative Suite</span>
            </div>
          </div>
        </div>

        {/* Dedicated Tool Switcher (Creative Triad) */}
        <nav className="flex items-center bg-zinc-950 p-1 rounded-xl border border-zinc-800 shadow-inner">
          <button
            onClick={() => setActiveTool('photo')}
            className={`px-4 py-1.5 rounded-lg text-xs font-semibold flex items-center gap-2 transition ${
              activeTool === 'photo'
                ? 'bg-indigo-600 text-white shadow-md shadow-indigo-600/30'
                : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900'
            }`}
          >
            <ImageIcon className="w-4 h-4" /> Photo Studio
          </button>
          <button
            onClick={() => setActiveTool('audio')}
            className={`px-4 py-1.5 rounded-lg text-xs font-semibold flex items-center gap-2 transition ${
              activeTool === 'audio'
                ? 'bg-indigo-600 text-white shadow-md shadow-indigo-600/30'
                : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900'
            }`}
          >
            <Music className="w-4 h-4" /> Audio Cutter
          </button>
          <button
            onClick={() => setActiveTool('video')}
            className={`px-4 py-1.5 rounded-lg text-xs font-semibold flex items-center gap-2 transition ${
              activeTool === 'video'
                ? 'bg-indigo-600 text-white shadow-md shadow-indigo-600/30'
                : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900'
            }`}
          >
            <VideoIcon className="w-4 h-4" /> Video Editor
          </button>
        </nav>

        {/* Right Info & Native Platform Badge */}
        <div className="flex items-center gap-2">
          <span className="text-[11px] px-2 py-0.5 rounded bg-zinc-800 text-zinc-300 font-mono border border-zinc-700/50">
            FOSS • Local First
          </span>
        </div>
      </header>

      {/* Main Body: Shelf + Workspace */}
      <div className="flex-1 flex overflow-hidden">
        {/* Collapsible Left Asset Shelf */}
        {isShelfOpen && (
          <aside className="w-72 border-r border-zinc-800 bg-zinc-900/60 flex flex-col z-20">
            <div className="p-3 border-b border-zinc-800 flex items-center justify-between">
              <span className="text-xs font-bold text-zinc-300 uppercase tracking-wider flex items-center gap-1.5">
                <FolderOpen className="w-3.5 h-3.5 text-indigo-400" /> Asset Shelf
              </span>
              <button
                onClick={() => fileInputRef.current?.click()}
                className="px-2.5 py-1 rounded bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium flex items-center gap-1 shadow transition"
              >
                <Upload className="w-3 h-3" /> Import
              </button>
              <input
                ref={fileInputRef}
                type="file"
                multiple
                accept="image/*,audio/*,video/*"
                onChange={(e) => handleFiles(e.target.files)}
                className="hidden"
              />
            </div>

            {/* Filter Tabs */}
            <div className="flex border-b border-zinc-800/80 bg-zinc-900/90 text-[11px] font-medium text-zinc-400">
              {(['all', 'photo', 'audio', 'video'] as const).map((filter) => (
                <button
                  key={filter}
                  onClick={() => setShelfFilter(filter)}
                  className={`flex-1 py-1.5 text-center capitalize ${
                    shelfFilter === filter ? 'text-indigo-400 font-bold border-b border-indigo-500' : 'hover:text-zinc-200'
                  }`}
                >
                  {filter}
                </button>
              ))}
            </div>

            {/* Asset List */}
            <div
              className="flex-1 overflow-y-auto p-2 space-y-1.5"
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault();
                handleFiles(e.dataTransfer.files);
              }}
            >
              {filteredAssets.length === 0 ? (
                <div
                  onClick={() => fileInputRef.current?.click()}
                  className="h-44 border-2 border-dashed border-zinc-800 hover:border-zinc-700 rounded-xl m-2 flex flex-col items-center justify-center p-4 text-center cursor-pointer text-zinc-500 transition"
                >
                  <Upload className="w-6 h-6 mb-2 text-zinc-600" />
                  <span className="text-xs font-medium">Drop media files here or click to import</span>
                </div>
              ) : (
                filteredAssets.map((asset) => {
                  const isSelected = asset.id === activeAssetId;
                  return (
                    <div
                      key={asset.id}
                      onClick={() => handleAssetClick(asset)}
                      className={`group p-2 rounded-lg flex items-center justify-between cursor-pointer border transition ${
                        isSelected
                          ? 'bg-indigo-600/10 border-indigo-500/60 text-white'
                          : 'bg-zinc-800/30 border-zinc-800 hover:bg-zinc-800/60 text-zinc-300'
                      }`}
                    >
                      <div className="flex items-center gap-2.5 overflow-hidden">
                        <div className="w-8 h-8 rounded bg-zinc-800 flex items-center justify-center flex-shrink-0 text-zinc-400">
                          {asset.type === 'photo' && <ImageIcon className="w-4 h-4 text-indigo-400" />}
                          {asset.type === 'audio' && <Music className="w-4 h-4 text-amber-400" />}
                          {asset.type === 'video' && <VideoIcon className="w-4 h-4 text-emerald-400" />}
                        </div>
                        <div className="overflow-hidden">
                          <div className="text-xs font-medium truncate">{asset.name}</div>
                          <div className="text-[10px] text-zinc-500 flex items-center gap-1.5">
                            <span className="uppercase">{asset.type}</span>
                            {asset.duration && <span>• {Math.round(asset.duration)}s</span>}
                          </div>
                        </div>
                      </div>

                      <button
                        onClick={(e) => handleDeleteAsset(asset.id, e)}
                        className="opacity-0 group-hover:opacity-100 p-1 hover:text-rose-400 text-zinc-500 transition"
                        title="Remove from Shelf"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  );
                })
              )}
            </div>
          </aside>
        )}

        {/* Dedicated Central Workspace */}
        <main className="flex-1 flex overflow-hidden">
          {activeTool === 'photo' && <PhotoStudio asset={activeAsset} />}
          {activeTool === 'audio' && <AudioCutter asset={activeAsset} />}
          {activeTool === 'video' && <VideoEditor assets={assets} />}
        </main>
      </div>
    </div>
  );
}
